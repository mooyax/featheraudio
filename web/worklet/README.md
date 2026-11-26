# featheraudio for web pages and games (AudioWorklet)

The engine runs on the browser's audio thread: the C API built as a plain WebAssembly module
(`featheraudio.wasm`, no wasm-bindgen, so it runs inside an AudioWorklet), driven by
`featheraudio-processor.js`. The page uses `featheraudio.js`, with the same calls as the Expo
module; they return at once and reach the audio thread in one message per task.

```sh
web/worklet/build.sh                  # → web/worklet/dist/ (all features, about 480 KB)
web/worklet/build.sh xm,ogg,effects   # only what the game needs
```

```html
<script src="featheraudio/featheraudio.js"></script>
<script>
  const ctx = new AudioContext();
  const fa = await FeatherAudio.create(ctx, { base: "featheraudio/" });
  fa.node.connect(ctx.destination);         // an AudioWorkletNode: mix it with your own nodes

  const boom = fa.fromAudioBuffer(buffer);  // an AudioBuffer (decodeAudioData, OfflineAudioContext)
  const song = fa.load(await (await fetch("song.xm")).arrayBuffer());

  fa.play(song, { loop: true, bus: fa.createBus() });
  fa.setListener([0, 0, 0], [0, 0, -1]);    // x right, y up, -z ahead
  const h = fa.play(boom, { position: [8, 0, -3], velocity: [-20, 0, 0] });
  fa.isPlaying(h);                          // known within about 50 ms
</script>
```

- Sources: `load` (WAV / OGG / MP3 / MOD / IT / S3M / XM), `stream` (OGG / MP3 while playing),
  `fromSamples`, `fromAudioBuffer`. The bytes are copied to the audio thread.
- Ids (sources, handles, buses) are made on the page, so every call returns at once.
  `isPlaying`, `time` and `activeVoices` come from a report every 50 ms.
- `test.html` is a self-test (`run-test.mjs` runs it in Chromium, Firefox and WebKit).
