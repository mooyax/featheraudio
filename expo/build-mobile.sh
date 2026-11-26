#!/bin/sh
# Builds the featheraudio C API (capi/) as static libraries for iOS and Android and puts them
# into this Expo module (macOS / Linux version of build-mobile.ps1):
#   ios/FeatherAudio.xcframework                  (iOS device arm64 + simulator arm64)
#   android/libs/<ABI>/libfeatheraudio.a          (arm64-v8a, armeabi-v7a, x86_64, x86)
# and copies the header (capi/include/featheraudio.h) next to the native code.
#
#   expo/build-mobile.sh                    all features (tracker, ogg, mp3, effects)
#   expo/build-mobile.sh xm,ogg,effects     only these
#   FA_TARGETS=android expo/build-mobile.sh (or ios): one platform only
#
# Needs rustup / cargo only (no Xcode, no Android NDK): only static libraries are built, the
# app build (EAS, or Xcode / Gradle) links them.
set -e
here=$(cd "$(dirname "$0")" && pwd)
crate="$here/../capi"
features="$1"
android="aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a x86_64-linux-android:x86_64 i686-linux-android:x86"
ios="aarch64-apple-ios:ios-arm64 aarch64-apple-ios-sim:ios-arm64-simulator"
case "${FA_TARGETS:-all}" in
  android) ios="" ;;
  ios) android="" ;;
esac
targets=""
for p in $android $ios; do targets="$targets ${p%%:*}"; done
rustup target add $targets
export IPHONEOS_DEPLOYMENT_TARGET=15.1
for t in $targets; do
  echo "== cargo $t"
  if [ -n "$features" ]; then
    cargo rustc --release --lib --crate-type staticlib --manifest-path "$crate/Cargo.toml" --target "$t" --no-default-features --features "$features"
  else
    cargo rustc --release --lib --crate-type staticlib --manifest-path "$crate/Cargo.toml" --target "$t"
  fi
done
cp "$crate/include/featheraudio.h" "$here/ios/featheraudio.h"
cp "$crate/include/featheraudio.h" "$here/android/src/main/cpp/featheraudio.h"
for p in $android; do
  mkdir -p "$here/android/libs/${p#*:}"
  cp "$crate/target/${p%%:*}/release/libfeatheraudio.a" "$here/android/libs/${p#*:}/libfeatheraudio.a"
done
if [ -n "$ios" ]; then
  xcf="$here/ios/FeatherAudio.xcframework"
  rm -rf "$xcf"
  for p in $ios; do
    mkdir -p "$xcf/${p#*:}"
    cp "$crate/target/${p%%:*}/release/libfeatheraudio.a" "$xcf/${p#*:}/libfeatheraudio.a"
  done
  cat > "$xcf/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AvailableLibraries</key>
	<array>
		<dict>
			<key>BinaryPath</key>
			<string>libfeatheraudio.a</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64</string>
			<key>LibraryPath</key>
			<string>libfeatheraudio.a</string>
			<key>SupportedArchitectures</key>
			<array>
				<string>arm64</string>
			</array>
			<key>SupportedPlatform</key>
			<string>ios</string>
		</dict>
		<dict>
			<key>BinaryPath</key>
			<string>libfeatheraudio.a</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64-simulator</string>
			<key>LibraryPath</key>
			<string>libfeatheraudio.a</string>
			<key>SupportedArchitectures</key>
			<array>
				<string>arm64</string>
			</array>
			<key>SupportedPlatform</key>
			<string>ios</string>
			<key>SupportedPlatformVariant</key>
			<string>simulator</string>
		</dict>
	</array>
	<key>CFBundlePackageType</key>
	<string>XFWK</string>
	<key>XCFrameworkFormatVersion</key>
	<string>1.0</string>
</dict>
</plist>
PLIST
fi
echo "== done"
find "$here/android/libs" "$here/ios" -name '*.a' -exec ls -l {} \; 2>/dev/null || true
