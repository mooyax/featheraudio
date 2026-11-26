# feather-audio — featheraudio for Expo (iOS / Android)

English | [日本語](README.ja.md)

A local Expo native module that brings the featheraudio engine to React Native apps: fire-and-forget sounds, buses, fades, effects (filters, echo, reverb), simple 3D sound and tracker music, with one JavaScript API on iOS and Android.

- iOS: Swift calls the C API (`ios/featheraudio.h`) and feeds an `AVAudioSourceNode` in `AVAudioEngine`.
- Android: Kotlin calls the C API through JNI (`android/src/main/cpp/featheraudio_jni.c`) and writes to an `AudioTrack` (float, low-latency mode) from its own thread.
- The engine itself is the Rust static library built from `capi/`.

Status: the C API and the JNI bridge are tested on Linux (a C test program and a Java program calling every JNI function). The Swift and Kotlin code has **not yet been built on a device**: please report problems.

## 1. Build the libraries

Only rustup and cargo are needed (no Xcode, no Android NDK: these are static libraries, linked by the app build).

```powershell
powershell -ExecutionPolicy Bypass -File expo\build-mobile.ps1
```

To ship less, choose features (see the main README): `-Features "xm,ogg,effects"`.

Output: `android/libs/<ABI>/libfeatheraudio.a` and `ios/FeatherAudioCore.xcframework`.

## 2. Add it to the app

1. Copy this folder into the app's `modules/` folder, e.g. `modules/feather-audio`. Expo autolinking finds `expo-module.config.json`.
2. To load tracker files as assets, add their extensions to Metro (`metro.config.js`): `config.resolver.assetExts.push("mod", "it", "s3m", "xm");`
3. Build the app as usual (`npx expo run:ios`, `npx expo run:android`, `eas build`).

## 3. Use it

```ts
import { Asset } from "expo-asset";
import * as FA from "../modules/feather-audio";

FA.init();

const [pewAsset, songAsset] = await Asset.loadAsync([require("./assets/pew.ogg"), require("./assets/song.xm")]);
const pew = FA.load(pewAsset.localUri!);            // WAV / OGG / MP3 / MOD / IT / S3M / XM
const song = FA.load(songAsset.localUri!);          // modules always loop

FA.play(pew);                                       // fire and forget
FA.playClocked(performance.now() / 1000, pew);      // rapid fire, evenly spaced

const music = FA.createBus();
const bgm = FA.play(song, { bus: music, protect: true, volume: 0 });
FA.fadeVolume(bgm, 0.8, 1.0);

// pause menu: muffle the music
FA.setFilter(FA.bus(music), 0, FA.FilterKind.LowPass, 20000);
FA.setEffectParam(FA.bus(music), 0, FA.Param.FILTER_CUTOFF, 20000);
FA.fadeEffectParam(FA.bus(music), 0, FA.Param.FILTER_CUTOFF, 800, 0.3);

// a cave
FA.setReverb(FA.bus(FA.MAIN_BUS), 0, { roomSize: 0.8 });

// 3D
FA.setListener([0, 0, 0], [0, 0, -1]);
const drone = FA.play(engineSound, { loop: true, position: [10, 0, -5], minDistance: 2, maxDistance: 100 });
FA.setPosition(drone, [x, 0, z]);                   // every frame
```

Long music in OGG / MP3: `FA.stream(uri)` decodes while playing and uses little memory.

## Notes

- The audio session (iOS) and audio focus (Android) are left to the app, so featheraudio can share the output with other audio modules (expo-audio and the like).
- All calls are synchronous and cheap: they only queue a command for the audio thread.
- Handles and source ids are plain numbers. Effect targets are a handle, or `FA.bus(n)` for a bus.
