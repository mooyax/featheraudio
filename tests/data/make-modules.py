#!/usr/bin/env python3
"""Makes the tracker modules for the end-of-song tests: the same little song as MOD, XM and
IT (loop-end.mod / .xm / .it). Everything is written here, so the files are free to use.

  python3 tests/data/make-modules.py

The song: orders [0, 1], 16 rows each, speed 6, tempo 125 (0.12 s a row). Pattern 0 has a
pattern loop: row 4 sets the loop start, row 7 jumps back twice, so rows 4..7 play three
times (16 + 8 rows). Pattern 1 plays straight. 40 rows: the song ends after 4.8 s, and then
would start again at order 0. A looped square wave is struck on every 4th row, so there is
sound up to the end. (MOD patterns have 64 rows: a pattern break on row 15 ends them.)
"""
import os
import struct

OUT = os.path.dirname(os.path.abspath(__file__))
WAVE = bytes([0x60] * 16 + [0xA0] * 16)  # a 32-byte square wave, 8-bit signed


def cell_effects(pattern, row):
    """(effect letter, param) on the first channel, or None"""
    if pattern == 0 and row == 4:
        return ("loopstart", 0)
    if pattern == 0 and row == 7:
        return ("loop", 2)
    if row == 15:
        return ("break", 0)
    return None


# ------------------------------------------------------------------ MOD

def make_mod():
    d = bytearray(b"loop end".ljust(20, b"\0"))
    d += b"square".ljust(22, b"\0") + struct.pack(">HBBHH", len(WAVE) // 2, 0, 48, 0, len(WAVE) // 2)
    for _ in range(30):
        d += bytes(22) + struct.pack(">HBBHH", 0, 0, 0, 0, 1)
    d += bytes([2, 127]) + bytes([0, 1]).ljust(128, b"\0") + b"M.K."
    for p in range(2):
        for r in range(64):
            for ch in range(4):
                n = bytearray(4)
                if ch == 0 and r < 16 and r % 4 == 0:
                    per = 428 if p == 0 else 381
                    n[0], n[1], n[2] = per >> 8, per & 0xFF, 0x10
                if ch == 1:
                    e = cell_effects(p, r)
                    if e:
                        kind, x = e
                        n[2] |= {"loopstart": 0xE, "loop": 0xE, "break": 0xD}[kind]
                        n[3] = {"loopstart": 0x60, "loop": 0x60 | x, "break": 0}[kind]
                d += n
    d += WAVE
    open(os.path.join(OUT, "loop-end.mod"), "wb").write(d)


# ------------------------------------------------------------------ XM

def make_xm():
    d = bytearray(b"Extended Module: " + b"loop end".ljust(20, b" ") + b"\x1a" + b"featheraudio".ljust(20, b" "))
    d += struct.pack("<HI", 0x0104, 276)
    d += struct.pack("<HHHHHHHH", 2, 0, 2, 2, 1, 1, 6, 125)
    d += bytes([0, 1]).ljust(256, b"\0")
    for p in range(2):
        rows = bytearray()
        for r in range(16):
            for ch in range(2):
                note = ins = vol = fx = par = 0
                if ch == 0 and r % 4 == 0:
                    note, ins = (49 if p == 0 else 51), 1  # C-4 / D-4
                if ch == 1:
                    e = cell_effects(p, r)
                    if e and e[0] != "break":  # 16-row patterns: no break needed
                        fx, par = 0xE, (0x60 if e[0] == "loopstart" else 0x60 | e[1])
                rows += bytes([note, ins, vol, fx, par])
        d += struct.pack("<IBHH", 9, 0, 16, len(rows)) + rows
    # instrument 1 with one sample
    ins = bytearray()
    ins += b"square".ljust(22, b"\0") + bytes([0]) + struct.pack("<H", 1)
    ins += struct.pack("<I", 40) + bytes(96) + bytes(48) + bytes(48)
    ins += bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]) + struct.pack("<H", 0) + bytes(2)
    ins = ins.ljust(263 - 4, b"\0")
    d += struct.pack("<I", 263) + ins
    d += struct.pack("<IIIBbBBbB", len(WAVE), 0, len(WAVE), 48, 0, 1, 128, 0, 0) + b"square".ljust(22, b"\0")
    prev, delta = 0, bytearray()
    for b in WAVE:
        v = b - 256 if b > 127 else b
        delta.append((v - prev) & 0xFF)
        prev = v
    d += delta
    open(os.path.join(OUT, "loop-end.xm"), "wb").write(d)


# ------------------------------------------------------------------ IT

def make_it():
    ordnum, insnum, smpnum, patnum = 3, 0, 1, 2  # orders 0, 1, end
    head = bytearray(b"IMPM" + b"loop end".ljust(26, b"\0") + struct.pack("<H", 0x1004))
    head += struct.pack("<HHHHHHHH", ordnum, insnum, smpnum, patnum, 0x0214, 0x0214, 0x0001, 0)
    head += bytes([128, 48, 6, 125, 128, 0]) + struct.pack("<HII", 0, 0, 0)
    head += bytes([32] * 2 + [0xA0] * 62) + bytes([64] * 64)
    head += bytes([0, 1, 255])
    offs_at = len(head)
    head += bytes(4 * (smpnum + patnum))
    # sample header
    smp_off = len(head)
    smp = bytearray(b"IMPS" + b"square.raw".ljust(12, b"\0") + b"\0" + bytes([64, 0x11, 48]) + b"square".ljust(26, b"\0"))
    smp += bytes([1, 32]) + struct.pack("<IIIIIII", len(WAVE), 0, len(WAVE), 8363, 0, 0, 0) + bytes(4)
    head += smp
    pat_offs = []
    for p in range(2):
        packed = bytearray()
        for r in range(16):
            if r % 4 == 0:
                packed += bytes([0x81, 0x03, 60 if p == 0 else 62, 1])  # channel 1: note + sample
            e = cell_effects(p, r)
            if e and e[0] != "break":
                packed += bytes([0x82, 0x08, 19, 0xB0 if e[0] == "loopstart" else 0xB0 | e[1]])  # S B x
            packed.append(0)
        pat_offs.append(len(head))
        head += struct.pack("<HH", len(packed), 16) + bytes(4) + packed
    data_off = len(head)
    head += WAVE
    struct.pack_into("<I", head, offs_at, smp_off)
    for i, o in enumerate(pat_offs):
        struct.pack_into("<I", head, offs_at + 4 * (smpnum + i), o)
    struct.pack_into("<I", head, smp_off + 0x48, data_off)
    open(os.path.join(OUT, "loop-end.it"), "wb").write(head)


make_mod()
make_xm()
make_it()
print("loop-end.mod / .xm / .it written")
