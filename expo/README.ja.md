# feather-audio — Expo 向けの featheraudio（iOS / Android）

[English](README.md) | 日本語

featheraudio のエンジンを React Native（Expo）のアプリで使うための、ローカルの Expo ネイティブモジュールです。鳴らしたら放っておける効果音、バス、フェード、エフェクト（フィルタ・エコー・リバーブ）、簡易 3D 定位、トラッカー音楽を、iOS と Android で同じ JavaScript の API から使えます。

- iOS：Swift が C API（`ios/featheraudio.h`）を呼び、`AVAudioEngine` の `AVAudioSourceNode` に音を流します。
- Android：Kotlin が JNI（`android/src/main/cpp/featheraudio_jni.c`）経由で C API を呼び、専用のスレッドから `AudioTrack`（浮動小数点、低遅延モード）に書き込みます。
- エンジン本体は `capi/` から作る Rust の静的ライブラリです。

状況：C API と JNI のつなぎは Linux で確認済みです（C のテストプログラムと、JNI の全関数を呼ぶ Java のプログラム）。Swift と Kotlin のコードは**まだ実機でビルドしていません**。問題があれば教えてください。

## 1. ライブラリを作る

rustup と cargo だけで作れます（静的ライブラリなので Xcode や Android NDK は不要で、リンクはアプリのビルドが行います）。

```powershell
powershell -ExecutionPolicy Bypass -File expo\build-mobile.ps1
```

サイズを減らしたいときは機能を選べます（本体の README を参照）：`-Features "xm,ogg,effects"`

できるもの：`android/libs/<ABI>/libfeatheraudio.a` と `ios/FeatherAudioCore.xcframework`

## 2. アプリに組み込む

1. このフォルダをアプリの `modules/` の下にコピーします（例：`modules/feather-audio`）。Expo が `expo-module.config.json` を見つけて組み込みます。
2. トラッカーの曲をアセットとして読むなら、Metro（`metro.config.js`）に拡張子を足します：`config.resolver.assetExts.push("mod", "it", "s3m", "xm");`
3. いつもどおりビルドします（`npx expo run:ios`、`npx expo run:android`、`eas build`）。

## 3. 使い方

```ts
import { Asset } from "expo-asset";
import * as FA from "../modules/feather-audio";

FA.init();

const [pewAsset, songAsset] = await Asset.loadAsync([require("./assets/pew.ogg"), require("./assets/song.xm")]);
const pew = FA.load(pewAsset.localUri!);            // WAV・OGG・MP3・MOD・IT・S3M・XM
const song = FA.load(songAsset.localUri!);          // 曲は常にループ

FA.play(pew);                                       // 鳴らしたら放っておく
FA.playClocked(performance.now() / 1000, pew);      // 連射しても間隔がそろう

const music = FA.createBus();
const bgm = FA.play(song, { bus: music, protect: true, volume: 0 });
FA.fadeVolume(bgm, 0.8, 1.0);

// ポーズ画面：BGM をこもらせる
FA.setFilter(FA.bus(music), 0, FA.FilterKind.LowPass, 20000);
FA.setEffectParam(FA.bus(music), 0, FA.Param.FILTER_CUTOFF, 20000);
FA.fadeEffectParam(FA.bus(music), 0, FA.Param.FILTER_CUTOFF, 800, 0.3);

// 洞窟の中
FA.setReverb(FA.bus(FA.MAIN_BUS), 0, { roomSize: 0.8 });

// 3D
FA.setListener([0, 0, 0], [0, 0, -1]);
const drone = FA.play(engineSound, { loop: true, position: [10, 0, -5], minDistance: 2, maxDistance: 100 });
FA.setPosition(drone, [x, 0, z]);                   // 毎フレーム
```

長い OGG・MP3 の曲は `FA.stream(uri)` で、鳴らしながら展開できます（メモリをほとんど使いません）。

## 補足

- 音声セッション（iOS）と音声フォーカス（Android）はアプリ側に任せています。expo-audio などほかの音のモジュールと出力を分け合えるようにするためです。
- どの呼び出しも同期で軽く、音声スレッドへの命令をキューに入れるだけです。
- ハンドルと音源の ID はただの数値です。エフェクトの対象は、ハンドルか、バスなら `FA.bus(n)` です。
