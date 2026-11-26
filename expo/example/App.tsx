// A test app for the featheraudio Expo module, laid out like a game's audio: music on a music
// bus (tracker module, or OGG / MP3 streamed), sound effects on an effects bus, 3D sounds,
// and effects. On start it checks the engine by itself (start, every file loads, sounds play,
// the engine clock moves) and prints one line "FEATHERAUDIO_SELFTEST PASS ..." or
// "... FAIL ..." to the log and the screen.
//
// Every sound here is made by make-sounds.py (no outside files).
import { useEffect, useRef, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { Asset } from "expo-asset";
import * as FA from "./modules/feather-audio/src";

const files = {
  song: require("./assets/song.mod"),
  musicOgg: require("./assets/music_ogg.ogg"),
  musicMp3: require("./assets/music_mp3.mp3"),
  pew: require("./assets/pew.wav"),
  coin: require("./assets/coin.wav"),
  boom: require("./assets/boom.wav"),
  chime: require("./assets/chime.wav"),
  engine: require("./assets/engine.wav"),
};
type Name = keyof typeof files;
type MusicKind = "song" | "musicOgg" | "musicMp3";
const MUSIC_LABEL: Record<MusicKind, string> = { song: "tracker (MOD)", musicOgg: "OGG, streamed", musicMp3: "MP3, streamed" };

async function uriOf(m: number): Promise<string> {
  const a = Asset.fromModule(m);
  await a.downloadAsync();
  if (!a.localUri) throw new Error("no local file for an asset");
  return a.localUri;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const rnd = (a: number, b: number) => a + Math.random() * (b - a);

export default function App() {
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState("starting…");
  const [live, setLive] = useState("");
  const [ready, setReady] = useState(false);
  const uris = useRef<Partial<Record<Name, string>>>({});
  const src = useRef<Partial<Record<Name, number>>>({});
  const bus = useRef({ music: 0, sfx: 0 });
  const music = useRef<{ h: number; kind: MusicKind | null; stream: number }>({ h: 0, kind: null, stream: 0 });
  const [musicKind, setMusicKind] = useState<MusicKind | null>(null);
  const [musicVol, setMusicVol] = useState(0.8);
  const [muffled, setMuffled] = useState(false);
  const [paused, setPaused] = useState(false);
  const [reverb, setReverb] = useState(false);
  const [echo, setEcho] = useState(false);
  const car = useRef<ReturnType<typeof setInterval> | null>(null);

  const log = (s: string) => {
    // iOS release builds pass only errors to the system log, where the CI looks for the
    // self-test line
    if (!__DEV__ && s.startsWith("FEATHERAUDIO_SELFTEST")) console.error(s);
    else console.log(s);
    setLines((l) => [...l.slice(-20), s]);
  };

  useEffect(() => {
    (async () => {
      const fail = (why: string) => {
        log(`FEATHERAUDIO_SELFTEST FAIL ${why}`);
        setStatus(`FAIL: ${why}`);
      };
      try {
        if (!FA.init()) return fail("init() returned false");
        log(`engine started, ${FA.sampleRate()} Hz`);
        bus.current = { music: FA.createBus(), sfx: FA.createBus() };
        FA.setBusVolume(bus.current.music, 0.8);
        // the music bus has a low-pass filter, wide open until "Muffle" (a pause menu)
        FA.setFilter(FA.bus(bus.current.music), 0, FA.FilterKind.LowPass, 20000, 0.8);
        FA.setEffectParam(FA.bus(bus.current.music), 0, FA.Param.FILTER_CUTOFF, 20000);
        for (const k of Object.keys(files) as Name[]) {
          uris.current[k] = await uriOf(files[k]);
          if (k === "musicOgg" || k === "musicMp3") continue; // streamed when played
          src.current[k] = FA.load(uris.current[k]!);
        }
        log(`loaded: ${Object.keys(src.current).join(", ")}`);
        // play a few at once, quietly, and see the clock move
        const t0 = FA.time();
        const hs = [
          FA.play(src.current.song!, { volume: 0.2, bus: bus.current.music }),
          FA.play(FA.stream(uris.current.musicOgg!), { volume: 0.2, bus: bus.current.music }),
          FA.play(FA.stream(uris.current.musicMp3!), { volume: 0.2, bus: bus.current.music }),
          FA.play(src.current.engine!, { volume: 0.2, loop: true }),
        ];
        await sleep(300);
        const voices = FA.activeVoices();
        await sleep(1200);
        const t1 = FA.time();
        hs.forEach(FA.stop);
        log(`clock ${t0.toFixed(2)} → ${t1.toFixed(2)} s, voices ${voices}`);
        if (!(t1 - t0 > 1.0)) return fail(`the engine clock did not move (${(t1 - t0).toFixed(2)} s): no audio output`);
        if (voices < 4) return fail(`only ${voices} of 4 sounds playing`);
        log(`FEATHERAUDIO_SELFTEST PASS rate=${FA.sampleRate()} clock=${(t1 - t0).toFixed(2)}`);
        setStatus("PASS: the engine runs and plays");
        setReady(true);
      } catch (e) {
        fail(String(e));
      }
    })();
    const timer = setInterval(() => setLive(`voices ${FA.activeVoices()} · clock ${FA.time().toFixed(1)} s`), 250);
    return () => {
      clearInterval(timer);
      if (car.current) clearInterval(car.current);
      FA.shutdown();
    };
  }, []);

  // ---- music ----

  const stopMusic = (fade = 0.5) => {
    const m = music.current;
    if (m.kind) {
      FA.fadeVolume(m.h, 0, fade);
      FA.stopAfter(m.h, fade);
      if (m.stream) {
        const s = m.stream;
        setTimeout(() => FA.unload(s), fade * 1000 + 200);
      }
    }
    music.current = { h: 0, kind: null, stream: 0 };
    setMusicKind(null);
  };
  const playMusic = (kind: MusicKind) => {
    stopMusic();
    let stream = 0;
    let source: number;
    if (kind === "song") source = src.current.song!;
    else source = stream = FA.stream(uris.current[kind]!);
    const h = FA.play(source, { bus: bus.current.music, loop: true, protect: true, volume: 0 });
    FA.fadeVolume(h, 1, 0.5);
    music.current = { h, kind, stream };
    setMusicKind(kind);
    log(`music: ${MUSIC_LABEL[kind]}`);
  };
  const changeVolume = (d: number) => {
    const v = Math.max(0, Math.min(1, Math.round((musicVol + d) * 10) / 10));
    setMusicVol(v);
    FA.fadeBusVolume(bus.current.music, v, 0.2);
  };
  const toggleMuffle = () => {
    FA.fadeEffectParam(FA.bus(bus.current.music), 0, FA.Param.FILTER_CUTOFF, muffled ? 20000 : 500, 0.4);
    setMuffled(!muffled);
  };
  const togglePause = () => {
    FA.setBusPaused(bus.current.music, !paused);
    setPaused(!paused);
  };

  // ---- sound effects ----

  const sfx = (k: Name, o: FA.PlayOptions = {}) => FA.play(src.current[k]!, { bus: bus.current.sfx, ...o });
  const pew = () => sfx("pew", { speed: rnd(0.85, 1.2), pan: rnd(-0.6, 0.6) });
  const rapid = () => {
    // 20 shots 45 ms apart, scheduled sample-accurately, and 4 explosions: many loud sounds
    // at once (the limiter keeps them from crackling)
    for (let i = 0; i < 20; i++) sfx("pew", { delay: i * 0.045, speed: rnd(0.9, 1.15), pan: rnd(-0.8, 0.8) });
    for (let i = 0; i < 4; i++) sfx("boom", { delay: i * 0.2, volume: 0.9 });
    log("rapid fire: 20 shots + 4 explosions");
  };

  // ---- 3D ----

  const carPass = () => {
    if (car.current) return;
    // drives past 6 m in front, from left to right at 25 m/s (90 km/h), with the Doppler shift
    let x = -60;
    const v = 25;
    const h = FA.play(src.current.engine!, { loop: true, position: [x, 0, -6], velocity: [v, 0, 0], minDistance: 2, maxDistance: 80 });
    car.current = setInterval(() => {
      x += v * 0.05;
      FA.setPosition(h, [x, 0, -6]);
      if (x > 60) {
        FA.stop(h);
        clearInterval(car.current!);
        car.current = null;
      }
    }, 50);
    log("car: left to right, in front");
  };
  const chimesAround = () => {
    // left, front, right, behind: 0.6 s apart
    const at: FA.Vec3[] = [
      [-6, 0, 0],
      [0, 0, -6],
      [6, 0, 0],
      [0, 0, 6],
    ];
    at.forEach((p, i) => FA.play(src.current.chime!, { delay: i * 0.6, position: p, minDistance: 1.5 }));
    log("chimes: left, front, right, behind");
  };

  // ---- effects ----

  const toggleReverb = () => {
    if (reverb) FA.removeEffect(FA.bus(FA.MAIN_BUS), 0);
    else FA.setReverb(FA.bus(FA.MAIN_BUS), 0, { roomSize: 0.8 });
    setReverb(!reverb);
  };
  const toggleEcho = () => {
    if (echo) FA.removeEffect(FA.bus(bus.current.sfx), 0);
    else FA.setEcho(FA.bus(bus.current.sfx), 0, { delay: 0.25, feedback: 0.45, damping: 0.3 });
    setEcho(!echo);
  };

  const off = !ready;
  return (
    <View style={st.root}>
      <Text style={st.title}>featheraudio</Text>
      <Text style={[st.status, status.startsWith("PASS") ? st.ok : status.startsWith("FAIL") ? st.ng : null]}>{status}</Text>
      <Text style={st.live}>{live}</Text>
      <ScrollView style={{ flex: 1 }} contentContainerStyle={{ paddingBottom: 24 }}>
        <Section title={`Music${musicKind ? ` — ${MUSIC_LABEL[musicKind]}` : ""}`}>
          <Row>
            <Btn label="Tracker" on={musicKind === "song"} off={off} onPress={() => playMusic("song")} />
            <Btn label="OGG" on={musicKind === "musicOgg"} off={off} onPress={() => playMusic("musicOgg")} />
            <Btn label="MP3" on={musicKind === "musicMp3"} off={off} onPress={() => playMusic("musicMp3")} />
            <Btn label="Stop" off={off} onPress={() => stopMusic()} />
          </Row>
          <Row>
            <Btn label="Vol −" off={off} onPress={() => changeVolume(-0.1)} />
            <Text style={st.value}>{Math.round(musicVol * 100)}%</Text>
            <Btn label="Vol +" off={off} onPress={() => changeVolume(0.1)} />
          </Row>
          <Row>
            <Btn label="Muffle" on={muffled} off={off} onPress={toggleMuffle} />
            <Btn label="Pause" on={paused} off={off} onPress={togglePause} />
          </Row>
        </Section>
        <Section title="Sound effects">
          <Row>
            <Btn label="Pew" off={off} onPress={pew} />
            <Btn label="Coin" off={off} onPress={() => sfx("coin")} />
            <Btn label="Boom" off={off} onPress={() => sfx("boom")} />
          </Row>
          <Row>
            <Btn label="Rapid fire ×20" off={off} onPress={rapid} />
            <Btn label="Echo" on={echo} off={off} onPress={toggleEcho} />
          </Row>
        </Section>
        <Section title="3D (headphones)">
          <Row>
            <Btn label="Car passes" off={off} onPress={carPass} />
            <Btn label="Chimes around" off={off} onPress={chimesAround} />
          </Row>
        </Section>
        <Section title="Everything">
          <Row>
            <Btn label="Reverb" on={reverb} off={off} onPress={toggleReverb} />
            <Btn
              label="Stop all"
              off={off}
              onPress={() => {
                stopMusic(0.05);
                FA.stopAll();
              }}
            />
          </Row>
        </Section>
        {lines.map((l, i) => (
          <Text key={i} style={st.line}>
            {l}
          </Text>
        ))}
      </ScrollView>
    </View>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <View style={st.section}>
      <Text style={st.sectionTitle}>{title}</Text>
      {children}
    </View>
  );
}

function Row({ children }: { children: React.ReactNode }) {
  return <View style={st.row}>{children}</View>;
}

function Btn({ label, onPress, on, off }: { label: string; onPress: () => void; on?: boolean; off?: boolean }) {
  return (
    <Pressable disabled={off} style={({ pressed }) => [st.btn, on && st.on, pressed && st.pressed, off && st.disabled]} onPress={onPress}>
      <Text style={st.btnText}>{label}</Text>
    </Pressable>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, backgroundColor: "#111323", paddingTop: 60, paddingHorizontal: 14 },
  title: { color: "#c8a24e", fontSize: 26, fontWeight: "600" },
  status: { color: "#ccc", fontSize: 15, marginTop: 4 },
  ok: { color: "#6c6" },
  ng: { color: "#e66" },
  live: { color: "#888", fontSize: 12, fontFamily: "monospace", marginBottom: 8 },
  section: { backgroundColor: "#191c30", borderRadius: 10, padding: 10, marginBottom: 10 },
  sectionTitle: { color: "#c8a24e", fontSize: 14, marginBottom: 8 },
  row: { flexDirection: "row", gap: 8, marginBottom: 8, alignItems: "center" },
  btn: { flex: 1, backgroundColor: "#262a44", paddingVertical: 13, borderRadius: 8, alignItems: "center" },
  on: { backgroundColor: "#4a3f1e" },
  pressed: { backgroundColor: "#3a3f66" },
  disabled: { opacity: 0.4 },
  btnText: { color: "#eee", fontSize: 14 },
  value: { color: "#eee", fontSize: 14, width: 48, textAlign: "center" },
  line: { color: "#888", fontFamily: "monospace", fontSize: 11 },
});
