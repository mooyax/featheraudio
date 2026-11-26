# featheraudio

English | [日本語](README.ja.md)

**[Try it in your browser →](https://mooyax.github.io/featheraudio/)** (3D sound, effects, and your own OGG / MP3 / WAV / tracker files, all mixed by the engine as WebAssembly)

A feather-light audio engine for games, in pure Rust. Inspired by [SoLoud](https://solhsa.com/soloud/): simple things are simple, and the harder things are possible.

- **Fire and forget**: `audio.play(&sound)` — the voice cleans itself up when it ends.
- **Handles** for every playing sound: volume, pan, speed, pause, loop, fade, scheduled stop.
- **Buses** (music, effects, UI…) with their own volume, fades and pause.
- **Voice limit** with protection: when all voices are in use, the oldest unprotected one makes room.
- **Tracker music** (MOD / IT / S3M / XM) with built-in engines, mixed together with your sound effects. Each format is a separate feature, so you only ship the engines you use. The same engines power the [modplay](https://github.com/mooyax/modplay-rs) player.
- **WAV, OGG Vorbis and MP3**: `Wav` decodes a file into memory (sound effects); `Stream` decodes while playing (music — a minute of stereo would otherwise take 23 MB). Streaming a 60 s file costs about 0.1% of one CPU core.
- **Sample-accurate timing**: `play_clocked` spreads sounds started during one audio buffer by their game times (rapid fire stays even, whatever the buffer size), `PlayOptions::delay` starts a sound exactly N seconds later, and `Audio::time()` is the engine's clock.
- **Effects**: low-pass / high-pass / band-pass / notch filters, echo and reverb, on a single sound or a whole bus (up to 4 per sound or bus), with smooth parameter fades — muffle the music when the pause menu opens with one call. Write your own by implementing `Effect`. Bus effects keep ringing after the sounds end.
- **Simple 3D sound**: a listener and positioned sounds, with distance attenuation (inverse, linear, exponential), left/right panning and Doppler shift. Computed in the mixer, so the game only sends positions.
- **Click-free**: volume changes are ramped; stop and pause fade out over a few milliseconds.
- **Real-time safe mixing**: the audio thread never locks or allocates. The game side and the mixer only talk through queues, and finished sounds are freed on the game side.
- **Other languages and platforms**: a C API (`capi/`, header `featheraudio.h`), an Expo module for React Native apps on iOS and Android (`expo/`), and a WebAssembly build with an interactive demo (`web/`).
- **Any output**: the default device through [cpal](https://github.com/RustAudio/cpal), or call `Mixer::render` yourself (your own audio callback, WebAssembly, offline rendering, tests).

Status: early (all 4 phases of the [plan](#roadmap) done; the Expo module still needs testing on devices). The API may still change.

## Example

```rust
use featheraudio::{Audio, PlayOptions, Stream, Tracker, Wav};

let audio = Audio::new()?;                         // default output device
let pew = Wav::from_bytes(&std::fs::read("pew.ogg")?)?;  // WAV, OGG or MP3
audio.play(&pew);                                  // fire and forget
audio.play_clocked(game_time, &pew);               // rapid fire, evenly spaced

let music = audio.create_bus().unwrap();
let song = Tracker::from_bytes(&std::fs::read("song.xm")?)?;
let bgm = audio.play_with(&song, PlayOptions::new().bus(music).protected(true));

let theme = Stream::from_vec(std::fs::read("theme.ogg")?)?; // decoded while playing
audio.play_with(&theme, PlayOptions::new().bus(music).looping(true).delay(1.0));

audio.fade_bus_volume(music, 0.4, 0.5);            // duck the music under a cutscene
audio.fade_volume(bgm, 0.0, 2.0);                  // ...and later fade it out
audio.stop_after(bgm, 2.0);
```

Effects and 3D:

```rust
use featheraudio::effects::{Filter, Reverb};
use featheraudio::{Bus, Spatial};

audio.set_effect(Bus::MAIN, 0, Reverb::new().room_size(0.7));      // a cave
audio.set_effect(music, 0, Filter::low_pass(20000.0));
audio.fade_effect_param(music, 0, Filter::CUTOFF, 800.0, 0.3);     // pause menu: muffle the music

audio.set_listener(camera_pos, camera_forward, [0.0, 1.0, 0.0]);
let car = audio.play_with(&engine, PlayOptions::new().looping(true)
    .spatial(Spatial::at(car_pos).distance(2.0, 100.0)));
audio.set_position(car, new_car_pos);                              // every frame
audio.set_velocity(car, car_velocity);                             // for the Doppler shift
```

Without an audio device (your own callback, offline rendering):

```rust
let (audio, mut mixer) = featheraudio::Audio::with_manual_output(48000);
audio.play(&pew);
let mut buf = vec![0.0f32; 1024 * 2];              // interleaved stereo
mixer.render(&mut buf);
```

### Examples in this repository

```
cargo run --release --example play -- sound.wav    # play a file on the default device
cargo run --release --example play -- music.ogg    # OGG / MP3 are streamed
cargo run --release --example play -- song.it      # modules loop until you press Enter
cargo run --release --example demo -- [music]      # render a short demo to demo.wav
cargo run --release --example bench_fx             # CPU cost of effects and 3D
```

## C API, Expo, WebAssembly

- **C API** — [`capi/`](capi): a static or shared library and the header [`capi/include/featheraudio.h`](capi/include/featheraudio.h) (documented there). `cargo build --release` in `capi/` gives `target/release/libfeatheraudio.a`. Functions start with `fa_`; handles and buses are numbers; effects go on a handle or `FA_BUS(n)`. Build with the `cpal` feature for `fa_audio_create()` (default device, desktop); without it, call `fa_audio_render()` from your own audio callback. [`capi/tests/test.c`](capi/tests/test.c) is a C program that tests it.
- **Expo (iOS / Android)** — [`expo/`](expo): a local Expo native module with a TypeScript API (`init`, `load`, `stream`, `play`, `setReverb`, `setPosition`…). See [expo/README.md](expo/README.md).
- **WebAssembly** — [`web/`](web): the engine in the browser, mixed in a Web Audio callback. The demo page places 3D sounds where you click, drives a car past you with the Doppler shift, toggles reverb and echo, muffles the music, and plays OGG / MP3 / WAV / tracker files you drop on it. `web/build.sh` (or `web\build-web.ps1` on Windows) builds it into `web/dist`; `--single` also writes one self-contained HTML file.

## Features (Cargo)

| Feature | Default | What it adds |
|---|---|---|
| `cpal` | yes | `Audio::new()`: output to the default device through cpal |
| `ogg` | yes | OGG Vorbis in `Wav` and `Stream` (through [lewton](https://github.com/RustAudio/lewton)) — about 140 KB |
| `mp3` | yes | MP3 in `Wav` and `Stream` (through [nanomp3](https://crates.io/crates/nanomp3), a pure Rust port of minimp3) — about 60 KB |
| `effects` | yes | the built-in effects `Filter`, `Echo`, `Reverb` — about 10 KB (the `Effect` trait, for your own effects, and 3D are always there) |
| `tracker` | yes | all three tracker engines below |
| `mod` | (with `tracker`) | ProTracker MOD engine, a port of pt2-clone (bit-identical output) — about 45 KB |
| `it` | (with `tracker`) | Impulse Tracker IT and Scream Tracker 3 S3M engine — about 100 KB |
| `xm` | (with `tracker`) | FastTracker 2 XM engine, a port of ft2-clone — about 60 KB |

With `default-features = false` the crate has no dependencies at all; use `Audio::with_manual_output`. For a game whose music is only XM: `default-features = false, features = ["cpal", "xm"]`.

(Sizes: what each engine adds to a stripped, LTO-optimized x86-64 build.)

## Concepts

- A **source** (`Wav`, `Stream`, `Tracker`, or your own type implementing `Source`) holds the data. Every `play` creates a new **instance** from it, so one source can play many times at once, even on top of itself.
- `Audio` is the game side: `Send + Sync`, every call just queues a command.
- `Mixer` is the audio side: it runs in the audio callback, applies the queued commands and mixes. It never locks and never allocates, except that `Stream` decoding allocates small buffers.
- Buses other than `Bus::MAIN` flow into the main bus, so the main bus's volume and effects apply to everything.
- **Effects** have numbered parameters: `effects::WET` (0 = bypassed, 1 = full effect) works on every effect and is handled by the engine; each effect type adds its own constants (`Filter::CUTOFF`, `Echo::FEEDBACK`, `Reverb::ROOM_SIZE`…). An effect's buffers are allocated on the game side, in `set_effect`.
- **3D**: positions are `[x, y, z]`; the listener starts at the origin facing -Z with +Y up (as in OpenAL and SoLoud). A 3D sound's pan comes from its direction (equal power, unchanged straight ahead), so `pan` is not used. Mono sources work best.
- Your own sources: implement `Source::instance` and `Instance::render` (fill interleaved stereo frames; return fewer frames than asked for when finished).

## Roadmap

1. **Core** (done): voices, handles, buses, fades, voice limit, WAV, tracker music (engines built in, one feature per format)
2. **Formats and timing** (done): OGG / MP3 (in memory and streamed), `play_clocked`, delayed starts, engine clock
3. **Effects and 3D** (done): filters, echo, reverb (Freeverb), effects on sounds and buses with parameter fades; distance attenuation, panning and Doppler
4. **Other platforms** (done): C API, Expo module for iOS / Android, WebAssembly demo

## Acknowledgements

- [SoLoud](https://solhsa.com/soloud/) by Jari Komppa — the model for the API and its "fire and forget" philosophy. No SoLoud code is used.
- [pt2-clone](https://github.com/8bitbubsy/pt2-clone) and [ft2-clone](https://github.com/8bitbubsy/ft2-clone) by Olav Sørensen (8bitbubsy) — the MOD and XM engines are ports of their replayers.
- [Freeverb](https://ccrma.stanford.edu/~jos/pasp/Freeverb.html) by Jezar at Dreampoint (public domain) — the reverb's design and tunings. The filters follow Robert Bristow-Johnson's Audio EQ Cookbook; distance and Doppler follow the OpenAL specification.
- [OpenMPT / libopenmpt](https://openmpt.org/) — the reference for the IT / S3M engine, whose resonant filters and mix-level tables follow OpenMPT.
- [cpal](https://github.com/RustAudio/cpal) — audio output; [lewton](https://github.com/RustAudio/lewton) — OGG Vorbis; [nanomp3](https://crates.io/crates/nanomp3), a port of [minimp3](https://github.com/lieff/minimp3) — MP3.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option, except the tracker engines in `src/tracker/` (features `mod`, `it`, `xm`), which are derived from pt2-clone, ft2-clone and OpenMPT and are under the [BSD 3-Clause license](LICENSE-BSD).
