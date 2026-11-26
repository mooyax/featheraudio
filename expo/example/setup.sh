#!/bin/sh
# Puts the Expo module (built with ../build-mobile.sh first) into modules/feather-audio, as
# in a game, and the test sounds into assets/.
set -e
here=$(cd "$(dirname "$0")" && pwd)
rm -rf "$here/modules/feather-audio"
mkdir -p "$here/modules/feather-audio" "$here/assets"
cd "$here/.."
cp -R package.json expo-module.config.json FeatherAudio.podspec src ios android "$here/modules/feather-audio/"
# Android keeps assets as resources named without the extension: the names must differ
for ext in ogg mp3 mp2; do cp "../tests/data/stereo.$ext" "$here/assets/test_$ext.$ext"; done
echo "module and test sounds in place"
