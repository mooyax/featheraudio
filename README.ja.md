# featheraudio

[English](README.md) | 日本語

Rust だけで書いた、ゲーム向けの羽のように軽いオーディオエンジンです。[SoLoud](https://solhsa.com/soloud/) にならい、「簡単なことは簡単に、難しいことも不可能にしない」作りを目指しています。

- **鳴らしたら放っておける**：`audio.play(&sound)` だけ。鳴り終わった音は自動で片付きます。
- **ハンドル**：鳴っている音ごとに、音量・パン・速さ・一時停止・ループ・フェード・時間指定の停止を操作できます。
- **バス**（BGM・効果音・UI など）：まとめて音量・フェード・一時停止できます。
- **同時発音数の上限**と保護：全部使っているときは、保護されていない一番古い音が場所を譲ります。
- **トラッカー音楽**（MOD / IT / S3M / XM）：エンジンを内蔵していて、効果音と一緒に混ぜます。形式ごとに feature が分かれているので、使うエンジンだけを入れられます。同じエンジンで [modplay](https://github.com/mooyax/modplay-rs) プレイヤーも動いています。
- **WAV** の読み込み（8 / 16 / 24 / 32 ビット整数、32 / 64 ビット浮動小数点）。
- **プチッというノイズが出ない**：音量の変化はなめらかにつなぎ、停止や一時停止も数ミリ秒でフェードアウトします。
- **リアルタイムでも安全なミキシング**：音を作るスレッドはロックもメモリ確保もしません。ゲーム側とミキサーはキューだけでやり取りし、鳴り終わった音の後片付けはゲーム側で行います。
- **出力先を選べる**：[cpal](https://github.com/RustAudio/cpal) で既定の音声デバイスに出すか、`Mixer::render` を自分で呼びます（独自のオーディオコールバック、WebAssembly、ファイルへの書き出し、テストなど）。

状況：作り始めたところです（[計画](#計画)の第 1 段階）。API はまだ変わることがあります。

## 使い方

```rust
use featheraudio::{Audio, PlayOptions, Tracker, Wav};

let audio = Audio::new()?;                         // 既定の音声デバイス
let pew = Wav::from_bytes(&std::fs::read("pew.wav")?)?;
audio.play(&pew);                                  // 鳴らしたら放っておく

let music = audio.create_bus().unwrap();
let song = Tracker::from_bytes(&std::fs::read("song.xm")?)?;
let bgm = audio.play_with(&song, PlayOptions::new().bus(music).protected(true));

audio.fade_bus_volume(music, 0.4, 0.5);            // ムービー中は BGM を下げる
audio.fade_volume(bgm, 0.0, 2.0);                  // あとでフェードアウトして
audio.stop_after(bgm, 2.0);                        // 止める
```

音声デバイスを使わない場合（自分のコールバック、ファイルへの書き出し）：

```rust
let (audio, mut mixer) = featheraudio::Audio::with_manual_output(48000);
audio.play(&pew);
let mut buf = vec![0.0f32; 1024 * 2];              // 左右交互のステレオ
mixer.render(&mut buf);
```

### このリポジトリの例

```
cargo run --release --example play -- sound.wav    # ファイルを既定のデバイスで鳴らす
cargo run --release --example play -- song.it      # 曲は Enter を押すまでループ
cargo run --release --example demo -- [song.xm]    # 短いデモを demo.wav に書き出す
```

## 機能（Cargo の features）

| 名前 | 既定 | 追加されるもの |
|---|---|---|
| `cpal` | 有効 | `Audio::new()`：cpal で既定のデバイスに出力 |
| `tracker` | 有効 | 下の 3 つのトラッカーエンジンすべて |
| `mod` | （`tracker` で有効） | ProTracker の MOD エンジン。pt2-clone の移植で、出力は元と完全に同じ — 約 45 KB |
| `it` | （`tracker` で有効） | Impulse Tracker の IT と Scream Tracker 3 の S3M のエンジン — 約 100 KB |
| `xm` | （`tracker` で有効） | FastTracker 2 の XM エンジン。ft2-clone の移植 — 約 60 KB |

`default-features = false` にすると依存クレートはゼロです。その場合は `Audio::with_manual_output` を使います。曲が XM だけのゲームなら `default-features = false, features = ["cpal", "xm"]` のようにします。

（サイズは、x86-64 向けに LTO で最適化して不要部分を落とした場合に、各エンジンで増える量です。）

## しくみ

- **音源**（`Wav`、`Tracker`、または `Source` を実装した自分の型）がデータを持ちます。`play` のたびにそこから新しい**インスタンス**が作られるので、1 つの音源を同時に何回でも、重ねてでも鳴らせます。
- `Audio` はゲーム側です。`Send + Sync` で、どの呼び出しも命令をキューに入れるだけです。
- `Mixer` は音を作る側です。オーディオコールバックの中で動き、届いた命令を反映して音を混ぜます。
- 自分の音源を作るには、`Source::instance` と `Instance::render` を実装します（左右交互のステレオで埋め、終わったら頼まれたより少ないフレーム数を返します）。

## 計画

1. **コア**（完了）：ボイス、ハンドル、バス、フェード、同時発音数の上限、WAV、トラッカー音楽（エンジン内蔵、形式ごとの feature）
2. OGG / MP3 の読み込み、`play_clocked`（1 つのオーディオバッファの中でも、ゲームの時刻どおりに音を鳴らし始める）
3. エフェクト：ローパス／ハイパスフィルタ、エコー、リバーブ、簡易 3D 定位
4. C API（Expo / Flutter / Unity などから使う）、iOS / Android 向けのつなぎ、WebAssembly のデモ

## 謝辞

- [SoLoud](https://solhsa.com/soloud/)（Jari Komppa 氏）— API と「鳴らしたら放っておく」考え方のお手本です。SoLoud のコードは使っていません。
- [pt2-clone](https://github.com/8bitbubsy/pt2-clone) と [ft2-clone](https://github.com/8bitbubsy/ft2-clone)（Olav Sørensen 氏 / 8bitbubsy）— MOD と XM のエンジンは、それぞれの再生部分の移植です。
- [OpenMPT / libopenmpt](https://openmpt.org/) — IT・S3M エンジンの答え合わせの基準です。レゾナンスフィルタや音量の表は OpenMPT に合わせています。
- [cpal](https://github.com/RustAudio/cpal) — 音声出力。

## ライセンス

[Apache License 2.0](LICENSE-APACHE) と [MIT ライセンス](LICENSE-MIT) のどちらかを選んで使えます。ただし `src/tracker/` のトラッカーエンジン（feature の `mod`・`it`・`xm`）は pt2-clone・ft2-clone・OpenMPT の派生物なので、[BSD 3-Clause ライセンス](LICENSE-BSD)です。
