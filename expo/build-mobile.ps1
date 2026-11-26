# Builds the featheraudio C API (capi/) as static libraries for iOS and Android and puts them
# into this Expo module:
#   ios\FeatherAudioCore.xcframework              (iOS device arm64 + simulator arm64)
#   android\libs\<ABI>\libfeatheraudio.a          (arm64-v8a, armeabi-v7a, x86_64, x86)
# and copies the header (capi\include\featheraudio.h) next to the native code.
#
#   powershell -ExecutionPolicy Bypass -File D:\develop\featheraudio\expo\build-mobile.ps1
#   powershell -ExecutionPolicy Bypass -File D:\develop\featheraudio\expo\build-mobile.ps1 -Features "xm,ogg,effects"
#
#   -Features   build only these features of capi (default: all — tracker, ogg, mp3, effects).
#               e.g. "xm,ogg,effects" for a game whose music is XM and whose sounds are OGG.
#
# Needs rustup / cargo only (no Xcode, no Android NDK): static libraries are not linked here,
# the app build (EAS or Xcode / Gradle) links them.
param([string]$Features = '')

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$crate = Join-Path (Split-Path -Parent $here) 'capi'
$targetDir = Join-Path $crate 'target'

$android = [ordered]@{
  'aarch64-linux-android'   = 'arm64-v8a'
  'armv7-linux-androideabi' = 'armeabi-v7a'
  'x86_64-linux-android'    = 'x86_64'
  'i686-linux-android'      = 'x86'
}
$ios = [ordered]@{
  'aarch64-apple-ios'     = 'ios-arm64'
  'aarch64-apple-ios-sim' = 'ios-arm64-simulator'
}

Write-Host '== rustup targets'
rustup target add @($android.Keys + $ios.Keys)
if ($LASTEXITCODE -ne 0) { throw 'rustup target add failed' }

$env:IPHONEOS_DEPLOYMENT_TARGET = '15.1'
$extra = @()
if ($Features -ne '') { $extra += @('--no-default-features', '--features', $Features) }

foreach ($t in @($android.Keys + $ios.Keys)) {
  Write-Host "== cargo $t"
  # only the static library (capi also makes a shared one, which would need the NDK's linker)
  cargo rustc --release --lib --crate-type staticlib --manifest-path (Join-Path $crate 'Cargo.toml') --target $t @extra
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $t" }
}

# ---- header ----
$h = Join-Path $crate 'include\featheraudio.h'
Copy-Item $h (Join-Path $here 'ios\featheraudio.h') -Force
Copy-Item $h (Join-Path $here 'android\src\main\cpp\featheraudio.h') -Force

# ---- Android ----
foreach ($t in $android.Keys) {
  $dst = Join-Path $here ("android\libs\" + $android[$t])
  New-Item -ItemType Directory -Force -Path $dst | Out-Null
  Copy-Item (Join-Path $targetDir "$t\release\libfeatheraudio.a") (Join-Path $dst 'libfeatheraudio.a') -Force
}

# ---- iOS: an XCFramework of two static libraries (written by hand, no xcodebuild needed) ----
# not "libfeatheraudio.a": macOS file names ignore case, and the pod's own Swift library is
# libFeatherAudio.a, so the linker would never see the engine
$old = Join-Path $here 'ios\FeatherAudio.xcframework'
if (Test-Path $old) { Remove-Item -Recurse -Force $old }
$xcf = Join-Path $here 'ios\FeatherAudioCore.xcframework'
if (Test-Path $xcf) { Remove-Item -Recurse -Force $xcf }
foreach ($t in $ios.Keys) {
  $dst = Join-Path $xcf $ios[$t]
  New-Item -ItemType Directory -Force -Path $dst | Out-Null
  Copy-Item (Join-Path $targetDir "$t\release\libfeatheraudio.a") (Join-Path $dst 'libfeatheraudio_core.a') -Force
}
$plist = @'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AvailableLibraries</key>
	<array>
		<dict>
			<key>BinaryPath</key>
			<string>libfeatheraudio_core.a</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64</string>
			<key>LibraryPath</key>
			<string>libfeatheraudio_core.a</string>
			<key>SupportedArchitectures</key>
			<array>
				<string>arm64</string>
			</array>
			<key>SupportedPlatform</key>
			<string>ios</string>
		</dict>
		<dict>
			<key>BinaryPath</key>
			<string>libfeatheraudio_core.a</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64-simulator</string>
			<key>LibraryPath</key>
			<string>libfeatheraudio_core.a</string>
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
'@
# UTF-8 without BOM, LF line endings
[System.IO.File]::WriteAllText((Join-Path $xcf 'Info.plist'), $plist.Replace("`r`n", "`n"), (New-Object System.Text.UTF8Encoding($false)))

Write-Host ''
Write-Host '== done'
Get-ChildItem -Recurse (Join-Path $here 'android\libs'), $xcf -Filter *.a | ForEach-Object { '{0,10:N0}  {1}' -f $_.Length, $_.FullName }
