#!/bin/sh
# Puts the Expo module (built with ../build-mobile.sh first) into modules/feather-audio, as
# in a game. (The app reads its sounds from sounds/.)
set -e
here=$(cd "$(dirname "$0")" && pwd)
rm -rf "$here/modules/feather-audio"
mkdir -p "$here/modules/feather-audio"
cd "$here/.."
cp -R package.json expo-module.config.json FeatherAudio.podspec src ios android "$here/modules/feather-audio/"
echo "module in place"
