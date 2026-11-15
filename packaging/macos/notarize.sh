#!/usr/bin/env bash
# Notarize a signed app or DMG and staple the ticket (prompt 101).
#
#   packaging/macos/notarize.sh <Kairo.app|Kairo.dmg>
#
# Credentials, with the Electron release's secret names (docs/signing-setup.md
# in kairo-presenter): APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD, APPLE_TEAM_ID.
# On a developer Mac, KAIRO_NOTARY_PROFILE names a profile saved once with
# `xcrun notarytool store-credentials`, so no password is in the environment.
# Apple's service is outside this machine, so a transient failure is retried.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=config.sh
source "$here/config.sh"

subject="${1:?usage: notarize.sh <Kairo.app|Kairo.dmg>}"
[[ -e "$subject" ]] || die "nothing to notarize at $subject"
if [[ -n "${KAIRO_NOTARY_PROFILE:-}" ]]; then
  auth=(--keychain-profile "$KAIRO_NOTARY_PROFILE")
else
  for var in APPLE_ID APPLE_APP_SPECIFIC_PASSWORD APPLE_TEAM_ID; do
    [[ -n "${!var:-}" ]] || die "$var is not set; notarization needs KAIRO_NOTARY_PROFILE, or APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD and APPLE_TEAM_ID"
  done
  auth=(--apple-id "$APPLE_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID")
fi

stage="$(mktemp -d "${TMPDIR:-/tmp}/kairo-notary.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
if [[ -d "$subject" ]]; then
  upload="$stage/$(basename "$subject").zip"
  ditto -c -k --sequesterRsrc --keepParent "$subject" "$upload"
else
  upload="$subject"
fi

for attempt in 1 2 3; do
  if xcrun notarytool submit "$upload" "${auth[@]}" --wait --timeout 45m --output-format plist >"$stage/result.plist"; then
    status="$(/usr/libexec/PlistBuddy -c 'Print :status' "$stage/result.plist" 2>/dev/null || true)"
    id="$(/usr/libexec/PlistBuddy -c 'Print :id' "$stage/result.plist" 2>/dev/null || true)"
    if [[ "$status" == Accepted ]]; then
      break
    fi
    # A rejection is not transient: print Apple's log and stop.
    if [[ -n "$id" ]]; then
      xcrun notarytool log "$id" "${auth[@]}" >&2 || true
    fi
    die "notarization of $subject ended with status '${status:-unknown}'"
  fi
  echo "package-macos: notarytool failed (attempt $attempt of 3)" >&2
  [[ "$attempt" -lt 3 ]] || die "notarization of $subject failed three times"
  sleep 60
done

for attempt in 1 2 3; do
  xcrun stapler staple "$subject" && break
  [[ "$attempt" -lt 3 ]] || die "could not staple the ticket to $subject"
  sleep 30
done
xcrun stapler validate "$subject"
note "notarized and stapled $subject"
