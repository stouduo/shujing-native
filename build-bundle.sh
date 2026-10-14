#!/bin/bash
# 打包 数镜 Native.app(本机安装/分发)
set -e
cd "$(dirname "$0")"

VERSION=$(grep '^version' crates/app/Cargo.toml | sed 's/.*"\(.*\)"/\1/')
NAME="数镜 Native"
BUNDLE="/tmp/数镜 Native.app"

echo "==> release 构建"
cargo build --release -p shujing-app

echo "==> 组装 .app (v$VERSION)"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS"
cp target/release/shujing-app "$BUNDLE/Contents/MacOS/shujing-native"

cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>数镜 Native</string>
    <key>CFBundleDisplayName</key><string>数镜 Native</string>
    <key>CFBundleIdentifier</key><string>com.shujing.native</string>
    <key>CFBundleExecutable</key><string>shujing-native</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "==> 打包完成: $BUNDLE"
echo "    安装: cp -R '$BUNDLE' /Applications/ && 重命名为 数镜Native.app"
