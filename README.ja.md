# featheraudio

[English](README.md) | 日本語

Rust だけで書いた、ゲーム向けの羽のように軽いオーディオエンジンです。[SoLoud](https://solhsa.com/soloud/) にならい、「簡単なことは簡単に、難しいことも不可能にしない」作りを目指しています。

- **鳴らしたら放っておける**：`audio.play(&sound)` だけ。鳴り終わった音は自動で片付きます。
- **ハンドル**：鳴っている音ごとに、音量・パン・速さ・一時停止・ループ・フェード・時間指定の停止を操作できます。
- **バス**（BGM・効果音・UI など）：まとめて音量・フェード・一時停止できます。
- **同時発音数の上限**と保護：全部使っているときは、保護されていない一番古い音が場所を譲ります。
- **トラッカー音楽**（MOD / IT / S3M / XM）：エンジンを内蔵していて、効果音と一緒に混ぜます。形式ごとに feature が分かれているので、使うエンジンだけを入れられます。同じエンジンで [modplay](https://github.com/mooyax/modplay-rs) プレイヤーも動いています。
- **WAV・OGG Vorbis・MP3**：`Wav` はファイルを全部メモリに展開します（効果音向け）。`Stream` は鳴らしながら少しずつ展開します（BGM 向け。全部展開するとステレオ 1 分で 23 MB になります）。60 秒の曲を展開しながら鳴らしても、CPU は 1 コアの 0.1% ほどです。
- **サンプル単位で正確なタイミング**：`play_clocked` は、1 回の音声バッファの間に鳴らし始めた音を、ゲームの時刻の差のとおりにずらします（バッファの大きさによらず連射の間隔がそろいます）。`PlayOptions::delay` でちょうど N 秒後に鳴らし始められ、`Audio::time()` でエンジンの時計が読めます。
- **エフェクト**：ローパス・ハイパス・バンドパス・ノッチのフィルタ、エコー、リバーブを、1 つの音にもバス全体にもかけられます（音・バスごとに 4 つまで）。パラメータはなめらかにフェードでき、ポーズ画面で BGM をこもらせるのも 1 行です。`Effect` を実装すれば自作のエフェクトも使えます。バスのエフェクトは、音が鳴り終わった後も余韻が残ります。
- **簡易 3D 定位**：聞き手（リスナー）と音の位置から、距離による減衰（逆距離・直線・指数）、左右の定位、ドップラー効果を付けます。計算はミキサー側で行うので、ゲームは位置を送るだけです。
- **プチッというノイズが出ない**：音量の変化はなめらかにつなぎ、停止や一時停止も数ミリ秒でフェードアウトします。
- **リアルタイムでも安全なミキシング**：音を作るスレッドはロックもメモリ確保もしません。ゲーム側とミキサーはキューだけでやり取りし、鳴り終わった音の後片付けはゲーム側で行います。
- **ほかの言語・環境から使える**：C API（`capi/`、ヘッダーは `featheraudio.h`）、iOS・Android の React Native アプリ向けの Expo モジュール（`expo/`）、WebAssembly 版と遊べるデモ（`web/`）があります。
- **出力先を選べる**：[cpal](https://github.com/RustAudio/cpal) で既定の音声デバイスに出すか、`Mixer::render` を自分で呼びます（独自のオーディオコールバック、WebAssembly、ファイルへの書き出し、テストなど）。

状況：作り始めたところです（[計画](#計画)の 4 段階すべて完了。Expo モジュールは実機での確認待ち）。API はまだ変わることがあります。

## 使い方

```rust
use featheraudio::{Audio, PlayOptions, Stream, Tracker, Wav};

let audio = Audio::new()?;                         // 既定の音声デバイス
let pew = Wav::from_bytes(&std::fs::read("pew.ogg")?)?;  // WAV・OGG・MP3
audio.play(&pew);                                  // 鳴らしたら放っておく
audio.play_clocked(game_time, &pew);               // 連射しても間隔がそろう

let music = audio.create_bus().unwrap();
let song = Tracker::from_bytes(&std::fs::read("song.xm")?)?;
let bgm = audio.play_with(&song, PlayOptions::new().bus(music).protected(true));

let theme = Stream::from_vec(std::fs::read("theme.ogg")?)?; // 鳴らしながら展開
audio.play_with(&theme, PlayOptions::new().bus(music).looping(true).delay(1.0));

audio.fade_bus_volume(music, 0.4, 0.5);            // ムービー中は BGM を下げる
audio.fade_volume(bgm, 0.0, 2.0);                  // あとでフェードアウトして
audio.stop_after(bgm, 2.0);                        // 止める
```

エフェクトと 3D：

```rust
use featheraudio::effects::{Filter, Reverb};
use featheraudio::{Bus, Spatial};

audio.set_effect(Bus::MAIN, 0, Reverb::new().room_size(0.7));      // 洞窟の中
audio.set_effect(music, 0, Filter::low_pass(20000.0));
audio.fade_effect_param(music, 0, Filter::CUTOFF, 800.0, 0.3);     // ポーズ画面：BGM をこもらせる

audio.set_listener(camera_pos, camera_forward, [0.0, 1.0, 0.0]);
let car = audio.play_with(&engine, PlayOptions::new().looping(true)
    .spatial(Spatial::at(car_pos).distance(2.0, 100.0)));
audio.set_position(car, new_car_pos);                              // 毎フレーム
audio.set_velocity(car, car_velocity);                             // ドップラー効果用
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
cargo run --release --example play -- music.ogg    # OGG・MP3 は鳴らしながら展開
cargo run --release --example play -- song.it      # 曲は Enter を押すまでループ
cargo run --release --example demo -- [music]      # 短いデモを demo.wav に書き出す
cargo run --release --example bench_fx             # エフェクトと 3D の CPU 負荷を測る
```

## C API・Expo・WebAssembly

- **C API** — [`capi/`](capi)：静的ライブラリ・共有ライブラリと、ヘッダー [`capi/include/featheraudio.h`](capi/include/featheraudio.h)（説明はヘッダーの中）。`capi/` で `cargo build --release` すると `target/release/libfeatheraudio.a` ができます。関数名は `fa_` で始まり、ハンドルとバスは数値、エフェクトの対象はハンドルか `FA_BUS(n)` です。`cpal` 機能付きでビルドすると `fa_audio_create()`（既定のデバイス、PC 向け）が使え、付けなければ自分のオーディオコールバックから `fa_audio_render()` を呼びます。[`capi/tests/test.c`](capi/tests/test.c) が C のテストプログラムです。
- **Expo（iOS / Android）** — [`expo/`](expo)：TypeScript の API（`init`、`load`、`stream`、`play`、`setReverb`、`setPosition` など）を持つ、ローカルの Expo ネイティブモジュールです。[expo/README.ja.md](expo/README.ja.md) を見てください。
- **WebAssembly** — [`web/`](web)：ブラウザの中でエンジンを動かし、Web Audio のコールバックで音を混ぜます。デモでは、クリックした場所に 3D の音を置く、車がドップラー効果付きで通り過ぎる、リバーブ・エコーの切り替え、BGM をこもらせる、落とした OGG・MP3・WAV・トラッカー曲を鳴らす、ができます。`web/build.sh`（Windows では `web\build-web.ps1`）で `web/dist` に作り、`--single` を付けると 1 つの HTML ファイルにまとめた版もできます。

## 機能（Cargo の features）

| 名前 | 既定 | 追加されるもの |
|---|---|---|
| `cpal` | 有効 | `Audio::new()`：cpal で既定のデバイスに出力 |
| `ogg` | 有効 | `Wav` と `Stream` で OGG Vorbis を読む（[lewton](https://github.com/RustAudio/lewton) を使用）— 約 140 KB |
| `mp3` | 有効 | `Wav` と `Stream` で MP3 を読む（minimp3 を Rust に移した [nanomp3](https://crates.io/crates/nanomp3) を使用）— 約 60 KB |
| `effects` | 有効 | 組み込みエフェクト `Filter`・`Echo`・`Reverb` — 約 10 KB（自作エフェクト用の `Effect` トレイトと 3D は常に入っています） |
| `tracker` | 有効 | 下の 3 つのトラッカーエンジンすべて |
| `mod` | （`tracker` で有効） | ProTracker の MOD エンジン。pt2-clone の移植で、出力は元と完全に同じ — 約 45 KB |
| `it` | （`tracker` で有効） | Impulse Tracker の IT と Scream Tracker 3 の S3M のエンジン — 約 100 KB |
| `xm` | （`tracker` で有効） | FastTracker 2 の XM エンジン。ft2-clone の移植 — 約 60 KB |

`default-features = false` にすると依存クレートはゼロです。その場合は `Audio::with_manual_output` を使います。曲が XM だけのゲームなら `default-features = false, features = ["cpal", "xm"]` のようにします。

（サイズは、x86-64 向けに LTO で最適化して不要部分を落とした場合に、各エンジンで増える量です。）

## しくみ

- **音源**（`Wav`、`Stream`、`Tracker`、または `Source` を実装した自分の型）がデータを持ちます。`play` のたびにそこから新しい**インスタンス**が作られるので、1 つの音源を同時に何回でも、重ねてでも鳴らせます。
- `Audio` はゲーム側です。`Send + Sync` で、どの呼び出しも命令をキューに入れるだけです。
- `Mixer` は音を作る側です。オーディオコールバックの中で動き、届いた命令を反映して音を混ぜます。ロックもメモリ確保もしません（`Stream` の展開だけは小さなメモリ確保をします）。
- `Bus::MAIN` 以外のバスは MAIN に流れ込みます。MAIN の音量とエフェクトはすべての音にかかります。
- **エフェクト**のパラメータは番号で指定します。`effects::WET`（0 = 効果なし、1 = 効果いっぱい）はどのエフェクトにも使え、エンジンが処理します。そのほかはエフェクトの種類ごとの定数です（`Filter::CUTOFF`、`Echo::FEEDBACK`、`Reverb::ROOM_SIZE` など）。エフェクトのバッファはゲーム側の `set_effect` の中で確保します。
- **3D**：位置は `[x, y, z]` です。リスナーは最初、原点で -Z の方を向き、+Y が上です（OpenAL・SoLoud と同じ）。3D の音の左右は向きから決まる（等パワー、正面では変わらない）ので、`pan` は使いません。モノラルの音源が向いています。
- 自分の音源を作るには、`Source::instance` と `Instance::render` を実装します（左右交互のステレオで埋め、終わったら頼まれたより少ないフレーム数を返します）。

## 計画

1. **コア**（完了）：ボイス、ハンドル、バス、フェード、同時発音数の上限、WAV、トラッカー音楽（エンジン内蔵、形式ごとの feature）
2. **形式とタイミング**（完了）：OGG / MP3（全部展開と、鳴らしながら展開）、`play_clocked`、遅らせて鳴らす、エンジンの時計
3. **エフェクトと 3D**（完了）：フィルタ、エコー、リバーブ（Freeverb）、音・バスへのエフェクトとパラメータのフェード、距離による減衰・定位・ドップラー効果
4. **ほかの環境**（完了）：C API、iOS / Android 向けの Expo モジュール、WebAssembly のデモ

## 謝辞

- [SoLoud](https://solhsa.com/soloud/)（Jari Komppa 氏）— API と「鳴らしたら放っておく」考え方のお手本です。SoLoud のコードは使っていません。
- [pt2-clone](https://github.com/8bitbubsy/pt2-clone) と [ft2-clone](https://github.com/8bitbubsy/ft2-clone)（Olav Sørensen 氏 / 8bitbubsy）— MOD と XM のエンジンは、それぞれの再生部分の移植です。
- [Freeverb](https://ccrma.stanford.edu/~jos/pasp/Freeverb.html)（Jezar 氏 / Dreampoint、パブリックドメイン）— リバーブの構成と数値。フィルタは Robert Bristow-Johnson 氏の Audio EQ Cookbook、距離減衰とドップラー効果は OpenAL の仕様に従っています。
- [OpenMPT / libopenmpt](https://openmpt.org/) — IT・S3M エンジンの答え合わせの基準です。レゾナンスフィルタや音量の表は OpenMPT に合わせています。
- [cpal](https://github.com/RustAudio/cpal) — 音声出力。[lewton](https://github.com/RustAudio/lewton) — OGG Vorbis。[nanomp3](https://crates.io/crates/nanomp3)（[minimp3](https://github.com/lieff/minimp3) の移植）— MP3。

## ライセンス

[Apache License 2.0](LICENSE-APACHE) と [MIT ライセンス](LICENSE-MIT) のどちらかを選んで使えます。ただし `src/tracker/` のトラッカーエンジン（feature の `mod`・`it`・`xm`）は pt2-clone・ft2-clone・OpenMPT の派生物なので、[BSD 3-Clause ライセンス](LICENSE-BSD)です。
