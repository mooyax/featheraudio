# featheraudio

English | [日本語](README.ja.md)

A feather-light audio engine for games, in pure Rust. Inspired by [SoLoud](https://solhsa.com/soloud/): simple things are simple, and the harder things are possible.

- **Fire and forget**: `audio.play(&sound)` — the voice cleans itself up when it ends.
- **Handles** for every playing sound: volume, pan, speed, pause, loop, fade, scheduled stop.
- **Buses** (music, effects, UI…) with their own volume, fades and pause.
- **Voice limit** with protection: when all voices are in use, the oldest unprotected one makes room.
- **Tracker music** (MOD / IT / S3M / XM) with built-in engines, mixed together with your sound effects. Each format is a separate feature, so you only ship the engines you use. The same engines power the [modplay](https://github.com/mooyax/modplay-rs) player.
- **WAV, OGG Vorbis and MP3**: `Wav` decodes a file into memory (sound effects); `Stream` decodes while playing (music — a minute of stereo would otherwise take 23 MB). Streaming a 60 s file costs about 0.1% of one CPU core.
- **Sample-accurate timing**: `play_clocked` spreads sounds started during one audio buffer by their game times (rapid fire stays even, whatever the buffer size), `PlayOptions::delay` starts a sound exactly N seconds later, and `Audio::time()` is the engine's clock.
- **Click-free**: volume changes are ramped; stop and pause fade out over a few milliseconds.
- **Real-time safe mixing**: the audio thread never locks or allocates. The game side and the mixer only talk through queues, and finished sounds are freed on the game side.
- **Any output**: the default device through [cpal](https://github.com/RustAudio/cpal), or call `Mixer::render` yourself (your own audio callback, WebAssembly, offline rendering, tests).

Status: early (phase 2 of the [plan](#roadmap) done). The API may still change.

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
```

## Features (Cargo)

| Feature | Default | What it adds |
|---|---|---|
| `cpal` | yes | `Audio::new()`: output to the default device through cpal |
| `ogg` | yes | OGG Vorbis in `Wav` and `Stream` (through [lewton](https://github.com/RustAudio/lewton)) — about 140 KB |
| `mp3` | yes | MP3 in `Wav` and `Stream` (through [nanomp3](https://crates.io/crates/nanomp3), a pure Rust port of minimp3) — about 60 KB |
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
- Your own sources: implement `Source::instance` and `Instance::render` (fill interleaved stereo frames; return fewer frames than asked for when finished).

## Roadmap

1. **Core** (done): voices, handles, buses, fades, voice limit, WAV, tracker music (engines built in, one feature per format)
2. **Formats and timing** (done): OGG / MP3 (in memory and streamed), `play_clocked`, delayed starts, engine clock
3. Effects: low-pass / high-pass filters, echo, reverb; simple 3D positioning
4. C API (for Expo / Flutter / Unity…), iOS / Android glue, WebAssembly demo

## Acknowledgements

- [SoLoud](https://solhsa.com/soloud/) by Jari Komppa — the model for the API and its "fire and forget" philosophy. No SoLoud code is used.
- [pt2-clone](https://github.com/8bitbubsy/pt2-clone) and [ft2-clone](https://github.com/8bitbubsy/ft2-clone) by Olav Sørensen (8bitbubsy) — the MOD and XM engines are ports of their replayers.
- [OpenMPT / libopenmpt](https://openmpt.org/) — the reference for the IT / S3M engine, whose resonant filters and mix-level tables follow OpenMPT.
- [cpal](https://github.com/RustAudio/cpal) — audio output; [lewton](https://github.com/RustAudio/lewton) — OGG Vorbis; [nanomp3](https://crates.io/crates/nanomp3), a port of [minimp3](https://github.com/lieff/minimp3) — MP3.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option, except the tracker engines in `src/tracker/` (features `mod`, `it`, `xm`), which are derived from pt2-clone, ft2-clone and OpenMPT and are under the [BSD 3-Clause license](LICENSE-BSD).
