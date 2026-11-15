#!/usr/bin/env bash
# Import the Developer ID Application certificate into a temporary keychain on
# a CI runner and print the identity name (prompt 101).
#
#   identity="$(packaging/macos/import-certificate.sh <keychain-path>)"
#
# Reads CSC_LINK (base64 of the .p12) and CSC_KEY_PASSWORD, the Electron
# release's secret names. Prints nothing and exits 0 when CSC_LINK is empty,
# so the caller falls back to the ad-hoc build. Never prints secret values.
# Remove the keychain afterwards with: security delete-keychain <keychain-path>
set -euo pipefail
keychain="${1:?usage: import-certificate.sh <keychain-path>}"
if [[ -z "${CSC_LINK:-}" ]]; then
  exit 0
fi
[[ -n "${CSC_KEY_PASSWORD:-}" ]] || { echo "package-macos: CSC_LINK is set but CSC_KEY_PASSWORD is not" >&2; exit 1; }

work="$(mktemp -d "${TMPDIR:-/tmp}/kairo-cert.XXXXXX")"
trap 'rm -rf "$work"' EXIT
keychain_password="$(uuidgen)"
printf '%s' "$CSC_LINK" | base64 --decode >"$work/cert.p12"

security create-keychain -p "$keychain_password" "$keychain" >&2
security set-keychain-settings -lut 21600 "$keychain" >&2
security unlock-keychain -p "$keychain_password" "$keychain" >&2
security import "$work/cert.p12" -k "$keychain" -P "$CSC_KEY_PASSWORD" -f pkcs12 -T /usr/bin/codesign >&2
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
# Search this keychain first, keeping the existing ones.
existing=()
while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"
  existing+=("${line//\"/}")
done < <(security list-keychains -d user)
security list-keychains -d user -s "$keychain" "${existing[@]}"

identity="$(security find-identity -v -p codesigning "$keychain" | awk -F'"' '/Developer ID Application/{print $2; exit}')"
[[ -n "$identity" ]] || { echo "package-macos: the certificate holds no Developer ID Application identity" >&2; exit 1; }
printf '%s\n' "$identity"
