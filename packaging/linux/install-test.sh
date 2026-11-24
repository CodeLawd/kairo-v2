#!/usr/bin/env bash
# Clean-install checks for the Linux packages (prompt 103), as root in a fresh
# Ubuntu container or VM. Changes the system: run only on a test machine.
#
#   packaging/linux/install-test.sh --deb <kairo-presenter_x.y.z_amd64.deb>
#       --appimage <Kairo-x.y.z.AppImage> --version x.y.z [--electron-deb <old.deb>]
#
# 1. With --electron-deb: install the Electron release first, then the native
#    deb over it: apt must upgrade kairo-presenter in place, remove app.asar
#    and keep the user's data.
# 2. Install the deb with apt from a clean system (no FFmpeg anywhere): its
#    Depends must be enough. Check the files, desktop entry and *.kairo MIME
#    type, then verify-package.sh on /opt/Kairo (ldd: FFmpeg from the package).
# 3. Launch under Xvfb with Mesa's software Vulkan and OpenGL (no GPU): the app
#    keeps running, starts kairo-engine and loads the bundled NDI runtime.
#    Without DISPLAY, and with no usable Vulkan or OpenGL driver, it exits 1
#    with its explanation instead of crashing.
# 4. The AppImage, unpacked, starts the same way.
# 5. Purge: /opt/Kairo and the desktop entry go; the user's data stays.
# Test-only tools (Xvfb, Mesa drivers, procps) are installed here; they are
# not in the deb's Depends.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

deb=""; appimage=""; version=""; electron_deb=""
while (($#)); do
  case "$1" in
    --deb) deb="$(realpath "${2:?}")"; shift 2 ;;
    --appimage) appimage="$(realpath "${2:?}")"; shift 2 ;;
    --version) version="${2:?}"; shift 2 ;;
    --electron-deb) electron_deb="$(realpath "${2:?}")"; shift 2 ;;
    *) echo "usage: install-test.sh --deb <deb> --appimage <AppImage> --version x.y.z [--electron-deb <deb>]" >&2; exit 2 ;;
  esac
done
[[ -n "$deb" && -n "$appimage" && -n "$version" ]] || { echo "install-test: --deb, --appimage and --version are required" >&2; exit 2; }
[[ "$(id -u)" == 0 ]] || { echo "install-test: run as root on a test system" >&2; exit 2; }
fail() { echo "install-test FAIL: $*" >&2; exit 1; }
ok() { echo "install-test ok: $*"; }
export DEBIAN_FRONTEND=noninteractive
. /etc/os-release
echo "install-test: $PRETTY_NAME"

apt-get update -qq
if ldconfig -p | grep -E 'libav(codec|format|util)\.so' >/dev/null; then
  fail "this system already has FFmpeg libraries; the test needs one without"
fi

# The user whose data must survive.
user_home="$(mktemp -d)"
user_config="$user_home/.config"
data="$user_config/Kairo"

# 1. Electron first, when asked.
if [[ -n "$electron_deb" ]]; then
  apt-get install -y -qq --no-install-recommends "$electron_deb" >/dev/null
  [[ -f /opt/Kairo/resources/app.asar ]] || fail "the Electron deb did not install"
  mkdir -p "$data"
  echo "left by install-test.sh before the upgrade" >"$data/kairo-install-test-marker.txt"
  ok "Electron $(dpkg-query -W -f='${Version}' kairo-presenter) installed"
fi

# 2. The native deb, from apt, with only its own Depends.
install_flags=(-y --no-install-recommends)
if [[ -n "$electron_deb" ]]; then
  old_version="$(dpkg-query -W -f='${Version}' kairo-presenter)"
  if ! dpkg --compare-versions "$version" gt "$old_version"; then
    # Until prompt 104 sets the release version, the workspace's is lower.
    echo "install-test: WARNING: $version is not above Electron's $old_version; installing as a downgrade" >&2
    install_flags+=(--allow-downgrades)
  fi
fi
apt-get install "${install_flags[@]}" "$deb"
[[ "$(dpkg-query -W -f='${Version}' kairo-presenter)" == "$version" ]] || fail "kairo-presenter is not at $version"
[[ ! -e /opt/Kairo/resources/app.asar ]] || fail "Electron files remain after the upgrade (app.asar)"
[[ -z "$electron_deb" || -f "$data/kairo-install-test-marker.txt" ]] || fail "the upgrade removed the user's data"
if ldconfig -p | grep -E 'libav(codec|format|util)\.so' >/dev/null; then
  fail "installing the deb pulled in a system FFmpeg"
fi
[[ -x /usr/bin/kairo-presenter ]] || fail "no /usr/bin/kairo-presenter"
[[ "$(readlink -f /usr/bin/kairo-presenter)" == /opt/Kairo/kairo-presenter ]] || fail "/usr/bin/kairo-presenter points elsewhere"
desktop=/usr/share/applications/Kairo.desktop
[[ -f "$desktop" ]] || fail "no $desktop"
grep '^MimeType=.*application/x-kairo' "$desktop" >/dev/null || fail "Kairo.desktop lacks application/x-kairo"
grep '^Categories=AudioVideo;' "$desktop" >/dev/null || fail "Kairo.desktop is not in AudioVideo"
ok "installed $version$([[ -n "$electron_deb" ]] && echo ' over Electron, user data kept') with its Depends only; no system FFmpeg"

"$here/verify-package.sh" --app /opt/Kairo --version "$version"

# Test-only tools: X server, software Vulkan/OpenGL, MIME lookup, ps.
apt-get install -y -qq --no-install-recommends xvfb xauth mesa-vulkan-drivers libgl1-mesa-dri \
  shared-mime-info xdg-utils procps >/dev/null
update-mime-database /usr/share/mime >/dev/null 2>&1 || true
printf '{}\n' >"$user_home/service.kairo"
mime="$(env HOME="$user_home" XDG_CONFIG_HOME="$user_config" xdg-mime query filetype "$user_home/service.kairo")"
[[ "$mime" == application/x-kairo ]] || fail "*.kairo is '$mime', expected application/x-kairo"
ok "*.kairo is application/x-kairo"

run_env=(env HOME="$user_home" XDG_CONFIG_HOME="$user_config" XDG_DATA_HOME="$user_home/.local/share")

launch() { # launch LABEL COMMAND...: Kairo must keep running and start its engine
  local label="$1"; shift
  local log="$user_home/launch-$label.log"
  xvfb-run -a -s "-screen 0 1920x1080x24" "${run_env[@]}" "$@" >"$log" 2>&1 &
  local runner=$!
  sleep 25
  if ! pgrep -x kairo-presenter >/dev/null; then
    cat "$log" >&2
    fail "$label: Kairo is not running 25 s after launch"
  fi
  pgrep -x kairo-engine >/dev/null || { cat "$log" >&2; fail "$label: kairo-engine did not start"; }
  grep -rh "NDI runtime loaded" "$data/logs" >/dev/null 2>&1 || {
    cat "$log" >&2
    grep -rh "NDI" "$data/logs" >&2 2>/dev/null || true
    fail "$label: the bundled NDI runtime did not load"
  }
  ok "$label: running under Xvfb (software rendering), kairo-engine started, NDI runtime loaded"
  pkill -x kairo-presenter || true
  sleep 12
  pkill -x kairo-engine || true
  wait "$runner" 2>/dev/null || true
  rm -rf "$data/logs"
}

# 3. Launch.
launch deb /usr/bin/kairo-presenter

set +e
no_display="$("${run_env[@]}" env -u DISPLAY /usr/bin/kairo-presenter 2>&1)"
status=$?
set -e
[[ "$status" == 1 ]] || fail "without DISPLAY Kairo exited with $status, expected 1"
grep 'Kairo needs an X11 display' <<<"$no_display" >/dev/null || fail "without DISPLAY Kairo did not explain: $no_display"
ok "no DISPLAY: exits 1 with its explanation"

# No usable driver: no Vulkan ICD and no EGL vendor, so wgpu finds no adapter.
set +e
no_gpu="$(VK_ICD_FILENAMES=/nonexistent.json VK_DRIVER_FILES=/nonexistent.json \
  __EGL_VENDOR_LIBRARY_FILENAMES=/nonexistent.json timeout 60 \
  xvfb-run -a "${run_env[@]}" /usr/bin/kairo-presenter 2>&1)"
status=$?
set -e
pkill -x kairo-engine || true
[[ "$status" == 1 ]] || fail "with no Vulkan or OpenGL driver Kairo exited with $status, expected 1: $no_gpu"
grep 'Kairo could not open its window' <<<"$no_gpu" >/dev/null || fail "with no driver Kairo did not explain: $no_gpu"
ok "no Vulkan or OpenGL driver: exits 1 with its explanation"

# 4. The AppImage (unpacked: containers have no FUSE).
rm -rf "$user_home/appimage"
mkdir -p "$user_home/appimage"
(cd "$user_home/appimage" && "$appimage" --appimage-extract >/dev/null)
launch AppImage "$user_home/appimage/squashfs-root/AppRun"

# 5. Purge keeps the user's data.
mkdir -p "$data"
apt-get purge -y -qq kairo-presenter >/dev/null
[[ ! -e /opt/Kairo/kairo-presenter ]] || fail "/opt/Kairo remains after purge"
[[ ! -e "$desktop" ]] || fail "Kairo.desktop remains after purge"
[[ ! -e /usr/bin/kairo-presenter ]] || fail "/usr/bin/kairo-presenter remains after purge"
[[ -d "$data" ]] || fail "purging removed the user's data"
ok "purged: app, desktop entry and command gone; user data kept"

echo
echo "install-test PASS ($PRETTY_NAME)"
