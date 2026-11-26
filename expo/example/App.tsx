// A test app for the featheraudio Expo module. On start it checks the engine by itself
// (start, load OGG / MP3 / MP2, play, the engine clock moving) and prints one line
// "FEATHERAUDIO_SELFTEST PASS ..." or "... FAIL ..." to the log and the screen; the buttons
// then play the sounds by hand.
import { useEffect, useRef, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { Asset } from "expo-asset";
import * as FA from "./modules/feather-audio/src";

const files = {
  ogg: require("./assets/test_ogg.ogg"),
  mp3: require("./assets/test_mp3.mp3"),
  mp2: require("./assets/test_mp2.mp2"),
};
type Kind = keyof typeof files;

async function uriOf(m: number): Promise<string> {
  const a = Asset.fromModule(m);
  await a.downloadAsync();
  if (!a.localUri) throw new Error("no local file for an asset");
  return a.localUri;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export default function App() {
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState("starting…");
  const sources = useRef<Partial<Record<Kind, number>>>({});
  const uris = useRef<Partial<Record<Kind, string>>>({});
  const music = useRef<number>(-1);
  const [reverb, setReverb] = useState(false);
  const [muffled, setMuffled] = useState(false);

  const log = (s: string) => {
    console.log(s);
    setLines((l) => [...l.slice(-30), s]);
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
        for (const k of Object.keys(files) as Kind[]) {
          const uri = await uriOf(files[k]);
          uris.current[k] = uri;
          const id = FA.load(uri);
          sources.current[k] = id;
          log(`${k}: loaded, ${FA.duration(id).toFixed(3)} s`);
          if (!(FA.duration(id) > 0.2)) return fail(`${k} too short (${FA.duration(id)} s)`);
        }
        const t0 = FA.time();
        const h = FA.play(sources.current.ogg!, { volume: 0.5 });
        const s = FA.play(FA.stream(uris.current.mp3!), { volume: 0.5 });
        await sleep(300);
        const voices = FA.activeVoices();
        await sleep(1200);
        const t1 = FA.time();
        log(`clock ${t0.toFixed(2)} → ${t1.toFixed(2)} s, voices ${voices}`);
        FA.stop(h);
        FA.stop(s);
        if (!(t1 - t0 > 1.0)) return fail(`the engine clock did not move (${(t1 - t0).toFixed(2)} s): no audio output`);
        if (voices < 2) return fail(`only ${voices} sounds playing`);
        log(`FEATHERAUDIO_SELFTEST PASS rate=${FA.sampleRate()} clock=${(t1 - t0).toFixed(2)}`);
        setStatus("PASS: the engine runs and plays");
      } catch (e) {
        fail(String(e));
      }
    })();
    return () => FA.shutdown();
  }, []);

  const play = (k: Kind) => {
    const id = sources.current[k];
    if (id !== undefined) FA.play(id);
  };
  const toggleMusic = () => {
    if (music.current >= 0 && FA.isPlaying(music.current)) {
      FA.fadeVolume(music.current, 0, 0.5);
      FA.stopAfter(music.current, 0.5);
      music.current = -1;
    } else if (uris.current.mp3) {
      music.current = FA.play(FA.stream(uris.current.mp3), { loop: true, protect: true });
    }
  };
  const toggleReverb = () => {
    if (reverb) FA.removeEffect(FA.bus(FA.MAIN_BUS), 0);
    else FA.setReverb(FA.bus(FA.MAIN_BUS), 0, { roomSize: 0.8 });
    setReverb(!reverb);
  };
  const toggleMuffle = () => {
    if (muffled) FA.removeEffect(FA.bus(FA.MAIN_BUS), 1);
    else FA.setFilter(FA.bus(FA.MAIN_BUS), 1, FA.FilterKind.LowPass, 600);
    setMuffled(!muffled);
  };

  return (
    <View style={st.root}>
      <Text style={st.title}>featheraudio</Text>
      <Text style={[st.status, status.startsWith("PASS") ? st.ok : status.startsWith("FAIL") ? st.ng : null]}>{status}</Text>
      <View style={st.row}>
        <Btn label="OGG" onPress={() => play("ogg")} />
        <Btn label="MP3" onPress={() => play("mp3")} />
        <Btn label="MP2" onPress={() => play("mp2")} />
      </View>
      <View style={st.row}>
        <Btn label="Music loop" onPress={toggleMusic} />
        <Btn label={reverb ? "Reverb off" : "Reverb on"} onPress={toggleReverb} />
        <Btn label={muffled ? "Clear" : "Muffle"} onPress={toggleMuffle} />
      </View>
      <View style={st.row}>
        <Btn label="Stop all" onPress={() => FA.stopAll()} />
      </View>
      <ScrollView style={st.log}>
        {lines.map((l, i) => (
          <Text key={i} style={st.line}>
            {l}
          </Text>
        ))}
      </ScrollView>
    </View>
  );
}

function Btn({ label, onPress }: { label: string; onPress: () => void }) {
  return (
    <Pressable style={({ pressed }) => [st.btn, pressed && st.pressed]} onPress={onPress}>
      <Text style={st.btnText}>{label}</Text>
    </Pressable>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, backgroundColor: "#111323", paddingTop: 64, paddingHorizontal: 16 },
  title: { color: "#c8a24e", fontSize: 28, fontWeight: "600", marginBottom: 8 },
  status: { color: "#ccc", fontSize: 16, marginBottom: 16 },
  ok: { color: "#6c6" },
  ng: { color: "#e66" },
  row: { flexDirection: "row", gap: 8, marginBottom: 8 },
  btn: { flex: 1, backgroundColor: "#262a44", paddingVertical: 14, borderRadius: 8, alignItems: "center" },
  pressed: { backgroundColor: "#3a3f66" },
  btnText: { color: "#eee", fontSize: 15 },
  log: { marginTop: 12, flex: 1 },
  line: { color: "#999", fontFamily: "monospace", fontSize: 12 },
});
