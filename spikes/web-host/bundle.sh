#!/bin/bash
# Assemble and ad-hoc sign the macOS bundle CEF requires. Usage: bundle.sh [debug|release]
set -euo pipefail
cd "$(dirname "$0")"
P=${1:-debug}
CEF=$(dirname "$(find target/$P/build -type d -name 'Chromium Embedded Framework.framework' -maxdepth 5 | head -1)")
A=out/KairoWeb.app
rm -rf "$A"; mkdir -p "$A/Contents/MacOS" "$A/Contents/Frameworks"
plist() { cat > "$1" <<X
<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>$2</string><key>CFBundleIdentifier</key><string>$3</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleName</key><string>$2</string><key>LSUIElement</key><string>1</string><key>NSSupportsAutomaticGraphicsSwitching</key><true/></dict></plist>
X
}
cp "target/$P/host" "$A/Contents/MacOS/host"
plist "$A/Contents/Info.plist" host dev.kairo.spike.web
cp -R "$CEF/Chromium Embedded Framework.framework" "$A/Contents/Frameworks/"
for s in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  n="host Helper$s"; h="$A/Contents/Frameworks/$n.app"
  mkdir -p "$h/Contents/MacOS"; cp "target/$P/helper" "$h/Contents/MacOS/$n"
  id=$(echo "$s" | tr -dc 'A-Za-z' | tr 'A-Z' 'a-z')
  plist "$h/Contents/Info.plist" "$n" "dev.kairo.spike.web.helper${id:+.$id}"
  codesign -s - -f "$h" 2>/dev/null
done
codesign -s - -f "$A/Contents/Frameworks/Chromium Embedded Framework.framework" 2>/dev/null
codesign -s - -f "$A" 2>/dev/null
du -sh "$A"
