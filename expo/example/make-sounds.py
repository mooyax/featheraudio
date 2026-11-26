#!/usr/bin/env python3
"""Makes the test app's sounds (expo/example/sounds/): everything is synthesized here, so the
files are free to use. Needs numpy and ffmpeg (for the OGG and MP3 music).

  python3 expo/example/make-sounds.py

- song.mod      a 4-channel ProTracker module (tracker music, as in a game)
- music_ogg.ogg an arpeggio loop as OGG Vorbis, streamed by the app
- music_mp3.mp3 the same loop as MP3 (LAME tag: loops without a gap)
- pew.wav, coin.wav, boom.wav, chime.wav, engine.wav  sound effects (engine loops, for 3D)
"""
import os
import struct
import subprocess
import wave

import numpy as np

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sounds")
os.makedirs(OUT, exist_ok=True)
rng = np.random.default_rng(7)


def save_wav(name, x, rate, channels=1):
    x = np.clip(x, -1, 1)
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes((x * 32767).astype("<i2").tobytes())


# ---------------------------------------------------------------- sound effects

R = 22050


def t_of(sec, rate=R):
    return np.arange(int(sec * rate)) / rate


def pew():
    t = t_of(0.18)
    u = t / t[-1]
    ph = np.cumsum((1400 - 1100 * u) / R)
    return np.where(ph % 1 < 0.5, 1.0, -1.0) * 0.45 * (1 - u) ** 2


def coin():
    t = t_of(0.35)
    f = np.where(t < 0.07, 988.0, 1319.0)
    ph = np.cumsum(f / R)
    return np.where(ph % 1 < 0.5, 1.0, -1.0) * 0.3 * np.exp(-np.maximum(t - 0.07, 0) * 9)


def boom():
    t = t_of(0.9)
    n = rng.standard_normal(len(t))
    # low-passed noise (one-pole) with a falling cutoff, plus a low thump
    y = np.zeros_like(n)
    a = 0.0
    for i in range(len(n)):
        k = 0.02 + 0.25 * np.exp(-t[i] * 6)
        a += (n[i] - a) * k
        y[i] = a
    y = y / np.max(np.abs(y))
    thump = np.sin(2 * np.pi * np.cumsum(90 - 60 * t) / R)
    return (y * 0.8 + thump * 0.6) * np.exp(-t * 4) * 0.7


def chime():
    t = t_of(1.2)
    s = sum(a * np.sin(2 * np.pi * 880 * m * t) * np.exp(-t * 3 * m) for m, a in [(1, 1), (2.76, 0.5), (5.4, 0.25), (8.93, 0.12)])
    return s * 0.4 * np.minimum(t * 400, 1)


def engine():
    # one second, a whole number of cycles of everything: loops seamlessly
    t = t_of(1.0)
    saw = (t * 110) % 1 * 2 - 1
    saw2 = (t * 55) % 1 * 2 - 1
    return (saw * 0.6 + saw2 * 0.4) * 0.4 * (1 + 0.25 * np.sin(2 * np.pi * 10 * t))


for name, f in [("pew", pew), ("coin", coin), ("boom", boom), ("chime", chime), ("engine", engine)]:
    save_wav(name + ".wav", f(), R)

# ---------------------------------------------------------------- streamed music (OGG / MP3)

MR = 44100
hz = lambda m: 440 * 2 ** ((m - 69) / 12)
CHORDS = [[57, 60, 64, 69], [53, 57, 60, 65], [48, 52, 55, 60], [55, 59, 62, 67]]
PATTERN = [0, 1, 2, 3, 2, 1, 0, 2, 0, 1, 2, 3, 2, 3, 1, 2]


def music():
    step = 60 / 112 / 4
    total = int(step * 16 * 8 * MR)  # 8 bars, about 17 s
    out = np.zeros((total, 2))

    def add(start, sig, pan):
        idx = (start + np.arange(len(sig))) % total  # note tails wrap around: a seamless loop
        out[idx, 0] += sig * (1 - pan)
        out[idx, 1] += sig * pan

    for bar in range(8):
        chord = CHORDS[bar % 4]
        start = int(bar * step * 16 * MR)
        n = int(step * 16 * MR)
        t = np.arange(n) / MR
        env = np.minimum(t * 3, 1) * (1 - t / t[-1]) ** 0.4
        pad = (np.sin(2 * np.pi * hz(chord[0] - 12) * t) + 0.5 * np.sin(2 * np.pi * hz(chord[2] - 12) * t)) * 0.12 * env
        add(start, pad, 0.5)
        for k, p in enumerate(PATTERN):
            s0 = start + int(k * step * MR)
            t2 = np.arange(int(step * 3 * MR)) / MR
            f = hz(chord[p] + 12 + (12 if bar >= 4 and k % 4 == 3 else 0))
            tri = 1 - 4 * np.abs((t2 * f) % 1 - 0.5)
            add(s0, tri * 0.16 * np.exp(-t2 * 8), 0.3 if k % 2 == 0 else 0.7)
        # a soft bass on every beat
        for b in range(4):
            t3 = np.arange(int(step * 4 * MR)) / MR
            add(start + int(b * 4 * step * MR), np.sin(2 * np.pi * hz(chord[0] - 24) * t3) * 0.25 * np.exp(-t3 * 5), 0.5)
    return out / max(1.0, np.max(np.abs(out)) / 0.8)


wav_path = os.path.join(OUT, "_music.wav")
save_wav("_music.wav", music().reshape(-1), MR, channels=2)
ff = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", wav_path]
subprocess.run(ff + ["-c:a", "libvorbis", "-q:a", "3", os.path.join(OUT, "music_ogg.ogg")], check=True)
subprocess.run(ff + ["-c:a", "libmp3lame", "-q:a", "4", os.path.join(OUT, "music_mp3.mp3")], check=True)
os.remove(wav_path)

# ---------------------------------------------------------------- tracker module (ProTracker M.K.)

# periods for C-1 .. B-3 (finetune 0)
PERIODS = [856, 808, 762, 720, 678, 640, 604, 570, 538, 508, 480, 453,
           428, 404, 381, 360, 339, 320, 302, 285, 269, 254, 240, 226,
           214, 202, 190, 180, 170, 160, 151, 143, 135, 127, 120, 113]


def i8(x):
    return np.clip(np.round(x * 127), -128, 127).astype(np.int8).tobytes()


# A sample with a 32-byte cycle plays C-2 at 8287 Hz / 32 = 259 Hz (MIDI 60): so for these,
# C-1 .. B-3 is MIDI 48 .. 83; the bass has a 64-byte cycle (an octave lower: MIDI 36 .. 71).
c32 = np.arange(32) / 32
c64 = np.arange(64) / 64
samples = [
    ("lead square", i8(np.where(c32 < 0.25, 0.7, -0.7)), 40, (0, 16)),          # 1
    ("bass triangle", i8(1 - 4 * np.abs(c64 - 0.5)), 64, (0, 32)),              # 2
    ("kick", i8(np.sin(2 * np.pi * np.cumsum(np.linspace(180, 40, 2400)) / 8287) * np.exp(-np.arange(2400) / 700)), 64, None),  # 3
    ("hat", i8(rng.uniform(-1, 1, 900) * np.exp(-np.arange(900) / 150)), 36, None),  # 4
    ("pad sine", i8(np.sin(2 * np.pi * c32) * 0.9), 30, (0, 16)),                # 5
]


def note(midi, base, sample, effect=0, param=0):
    p = PERIODS[max(0, min(35, midi - base))]
    return bytes([(sample & 0xF0) | (p >> 8), p & 0xFF, ((sample & 0x0F) << 4) | effect, param])


EMPTY = bytes(4)


def pattern(chord_a, chord_b, fill):
    rows = []
    for r in range(64):
        chord = chord_a if r < 32 else chord_b
        rr = r % 32
        ch = [EMPTY] * 4
        # 1: bass on the beat and the off-beat
        if rr % 8 == 0:
            ch[0] = note(chord[0] - 12, 36, 2)
        elif rr % 8 == 6:
            ch[0] = note(chord[0], 36, 2, 0xC, 24)
        # 2: arpeggio
        if rr % 2 == 0:
            ch[1] = note(chord[PATTERN[(rr // 2) % 16]] + 12, 48, 1, 0xC, 36 if rr % 4 == 0 else 26)
        # 3: drums
        if rr % 8 == 0:
            ch[2] = note(60, 48, 3)
        elif rr % 4 == 2 or (fill and r >= 56 and rr % 2 == 0):
            ch[2] = note(72, 48, 4)
        # 4: pad, one note per half
        if rr == 0:
            ch[3] = note(chord[1], 48, 5, 0xC, 30)
        elif rr == 16:
            ch[3] = note(chord[2], 48, 5, 0xC, 22)
        rows.append(b"".join(ch))
    return b"".join(rows)


patterns = [pattern(CHORDS[0], CHORDS[1], False), pattern(CHORDS[2], CHORDS[3], True)]
mod = bytearray(b"featheraudio test".ljust(20, b"\0"))
for i in range(31):
    if i < len(samples):
        name, data, vol, loop = samples[i]
        ls, ll = loop if loop else (0, 1)
        mod += name.encode().ljust(22, b"\0") + struct.pack(">HBBHH", len(data) // 2, 0, vol, ls, ll)
    else:
        mod += bytes(22) + struct.pack(">HBBHH", 0, 0, 0, 0, 1)
order = [0, 1, 0, 1]
mod += bytes([len(order), 127]) + bytes(order).ljust(128, b"\0") + b"M.K."
for p in patterns:
    mod += p
for _, data, _, _ in samples:
    mod += data
open(os.path.join(OUT, "song.mod"), "wb").write(bytes(mod))

for f in sorted(os.listdir(OUT)):
    print(f"{os.path.getsize(os.path.join(OUT, f)):8d}  {f}")
