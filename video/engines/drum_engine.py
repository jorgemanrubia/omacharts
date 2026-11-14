#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "scipy", "soundfile"]
# ///
"""The Trading trailer's music engine: an original, bright electronic track.

    video/engines/drum_engine.py OUT_DIR

Writes OUT_DIR/drum.wav (48 kHz, 24-bit, stereo, mastered to -14 LUFS) and
OUT_DIR/hits.json, the frame, sounded time and level of every hit, which the
edit reads. (The name is the pipeline's; it began as the trailer's drum.)
Nothing is sampled: the drums are synthesised (a kick with a pitch envelope
and a click, a layered clap, noise and metal hats, a crash, an impact), the
bass, pads, supersaws and the pluck are band-limited saws through filters,
with sidechain pumping, a ping-pong delay and synthetic rooms. Deterministic
from seed 7.

The grid is the edit's: 60 fps, 26 frames a beat (138.46 BPM), bars of 4/4
from frame 52, a two-beat pickup in frames 0 to 51. Every 16 beats a card
slams on a bar's downbeat, frames 260 + 416k: a clean hit exactly on its
frame. Two beats later (+52) a lighter answer for the card's second line.
Each block is a lift of its own; the card at 2756 ("Agent-ready.") is the
drop; the hit at 4004 ends the track and rings out to silence.

Key of D minor, ending on F. Arrangement, one line a block (chords one a bar):

  0 pickup + 2 bars  Dm9 Dm9               the motif on a pluck from sample 0 with its delay, 16th hats,
                                            a filtered offbeat bass; a riser into the first card
  1 cover   260      Dm9 Bbmaj7 Gm9 A7sus4  impact; the kick four to the floor, pad, bass pumping, the
                                            motif; claps join in bar 3; snare eighths into
  2 Rithmic 676      Dm9 Bbmaj7 Gm9 A7sus4  crash; the full groove: claps on 2 and 4, 16th hats, open
                                            hats; motif filter wide open; a noise riser into
  3 chart   1092     Gm9 Bbmaj7 Dm9 A7sus4  crash and stab; supersaw chord stabs on the offbeats; toms into
  4 stop    1508     Dm9 Bbmaj7 Fmaj7 C     crash; a 16th arpeggio over it, the motif up an octave and
                                            doubled; reverse cymbal and a snare run into
  5 panel   1924     Bbmaj7 C Dm9 A7sus4    impact; rolling 16th bass, shaker; a one-beat stutter break into
  6 header  2340     Dm9 Bbmaj7 Gm9 A7sus4  crash; a counter-line; then the build: the band closes its
                                            filter, drums give way to an accelerating snare roll and a
                                            riser, and one beat of silence before
  7 DROP    2756     Dm9 Bbmaj7 Fmaj7 C     "Agent-ready.": the biggest lift: impact, supersaw lead on the
                                            motif, wide pads, deep pumping sub, everything
  8 agent   3172     Dm9 Bbmaj7 Fmaj7 C     crash; the motif's answer, higher; arpeggio; stutter into
  9 bracket 3588     Gm9 Bbmaj7 Csus4 C     crash; rising harmony to C, counter-line; a snare roll and a
                                            riser into
 10 end     4004     Fadd9                  the final hit: kick, impact, crash, the whole band on F,
                                            ringing out through its room to silence; no fade

Also leaves OUT_DIR/drum-raw.wav (the unmastered mix) and OUT_DIR/waveform.png
(the whole file, and the first video phrase up close), and prints the loudness
and onset checks.
"""

import json
import struct
import subprocess
import sys
import zlib
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, oaconvolve, resample_poly, sosfilt

SR = 48000
FPS = 60
SPF = SR // FPS  # samples per frame
FRAMES_PER_BEAT = 26
PHRASE_FRAMES = 16 * FRAMES_PER_BEAT
PHRASES = 10
TAIL_FRAMES = 170
FRAMES = PHRASES * PHRASE_FRAMES + TAIL_FRAMES
LENGTH = FRAMES * SPF
SIXTEENTH = FRAMES_PER_BEAT * SPF // 4
FIRST_BAR = 52 * SPF  # bar 0's downbeat; the pickup fills the two beats before it
SEED = 7

FADE_SECONDS = 0.3  # a safety fade over the last, already silent, moment
TARGET_I, TARGET_TP = -14.0, -1.0
KNEE = -7.0  # dBFS, where the master's soft knee starts


def db(x):
    return 10 ** (x / 20)


def seconds(n):
    return np.arange(int(n * SR)) / SR


def band(lo, hi, order=2):
    return butter(order, [lo, hi], btype="bandpass", fs=SR, output="sos")


def lowpass(x, fc, order=2):
    return sosfilt(butter(order, min(fc, 20000), btype="lowpass", fs=SR, output="sos"), x, axis=0)


def highpass(x, fc, order=2):
    return sosfilt(butter(order, fc, btype="highpass", fs=SR, output="sos"), x, axis=0)


def ramp(x, ms):
    """A raised-cosine onset, so nothing starts on a step."""
    n = max(1, int(ms * SR / 1000))
    x[:n] *= (0.5 - 0.5 * np.cos(np.pi * np.arange(n) / n)).reshape((-1,) + (1,) * (x.ndim - 1))
    return x


def tail_off(x, ms=20):
    n = min(len(x), int(ms * SR / 1000))
    x[-n:] *= np.linspace(1, 0, n).reshape((-1,) + (1,) * (x.ndim - 1))
    return x


def peak_to(x, level=1.0):
    return x * (level / np.max(np.abs(x)))


def noise(rng, n, sos=None):
    x = rng.standard_normal(n + 1024)
    return (sosfilt(sos, x) if sos is not None else x)[1024:]


def swept(x, cutoffs, kind="lowpass", chunk=240):
    """A filter whose cutoff moves: run in 5 ms steps, the state carried over."""
    out = np.empty_like(x)
    zi = None
    for i in range(0, len(x), chunk):
        fc = float(np.clip(cutoffs[min(i, len(cutoffs) - 1)], 40, 18000))
        sos = butter(2, [fc / 1.5, min(fc * 1.5, 22000)] if kind == "bandpass" else fc, btype=kind, fs=SR, output="sos")
        if zi is None:
            zi = np.zeros((sos.shape[0], 2))
        out[i : i + chunk], zi = sosfilt(sos, x[i : i + chunk], zi=zi)
    return out


def loud(x, level_db=-12):
    """Every sound leaves its maker equally loud: the RMS of its first 60 ms
    (or of all of it, if shorter) at -12 dB."""
    head = x[: int(0.06 * SR)]
    return x * (db(level_db) / np.sqrt(np.mean(head**2) + 1e-20))


def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)


def midi(name):
    """'F#5' -> 78, 'Bb4' -> 70."""
    pc = "C D EF G A B".index(name[0])
    rest = name[1:]
    while rest[0] in "#b":
        pc += 1 if rest[0] == "#" else -1
        rest = rest[1:]
    return 12 * (int(rest) + 1) + pc


# --- Oscillators and synths -------------------------------------------------------------------


def saw(f, n, phase=0.0):
    """A band-limited sawtooth (polyBLEP), at a fixed or moving frequency."""
    dt = np.broadcast_to(np.asarray(f, float) / SR, (n,))
    ph = (phase + np.cumsum(dt)) % 1.0
    y = 2 * ph - 1
    a = ph < dt
    x = ph[a] / dt[a]
    y[a] -= x + x - x * x - 1
    b = ph > 1 - dt
    x = (ph[b] - 1) / dt[b]
    y[b] -= x * x + x + x + 1
    return y


def envelope(n, attack, decay=None, sustain=1.0, length=None, release=0.05):
    """Attack, an exponential fall to the sustain level, and a release after length."""
    t = np.arange(n) / SR
    env = np.clip(t / max(attack, 1e-4), 0, 1)
    if decay:
        env = env * (sustain + (1 - sustain) * np.exp(-np.maximum(t - attack, 0) / decay))
    if length is not None:
        env = env * np.where(t < length, 1.0, np.exp(-(t - length) / release))
    return env


def pluck(rng, m, length, bright=1.0):
    """Two detuned saws through a low-pass whose cutoff snaps shut: the motif's voice."""
    n = int((length + 0.35) * SR)
    f = hz(m)
    x = saw(f * 2 ** (6 / 1200), n, rng.random()) + saw(f * 2 ** (-6 / 1200), n, rng.random())
    x += 0.5 * np.sin(2 * np.pi * f * np.arange(n) / SR)
    t = np.arange(n) / SR
    cutoff = f * 1.5 + 5200 * bright * np.exp(-t / 0.085)
    x = swept(x, cutoff)
    x *= envelope(n, 0.002, 0.32, 0.0, length, 0.06)
    return tail_off(loud(ramp(x, 1)), 10)


def supersaw(rng, m, length, cutoff=5000, attack=0.008, release=0.25, voices=7, spread=0.6):
    """Seven saws spread in pitch and across the stereo field."""
    n = int((length + release * 5) * SR)
    f = hz(m)
    out = np.zeros((n, 2))
    for i in range(voices):
        d = (i - (voices - 1) / 2) / ((voices - 1) / 2) if voices > 1 else 0
        y = saw(f * 2 ** (d * 18 / 1200), n, rng.random())
        pan = d * spread
        out[:, 0] += y * np.cos((pan + 1) * np.pi / 4)
        out[:, 1] += y * np.sin((pan + 1) * np.pi / 4)
    out = lowpass(highpass(out, 120), cutoff)
    out *= envelope(n, attack, None, 1, length, release)[:, None]
    held = out[int(attack * SR) : int(attack * SR) + int(0.1 * SR)]  # measured once the attack is over
    return tail_off(ramp(out, 2) * db(-12) / np.sqrt(np.mean(held**2)), 30)


def pad(rng, m, length, cutoff=1800):
    """A warm pad note: four slow saws, a soft attack and a long release."""
    return supersaw(rng, m, length, cutoff=cutoff, attack=0.35, release=0.5, voices=4, spread=0.9)


def bass(rng, m, length, cutoff=700):
    """A warm bass: a saw with a filter blip over a sine an octave down."""
    n = int((length + 0.2) * SR)
    f = hz(m)
    t = np.arange(n) / SR
    x = saw(f, n, 0.5) * 0.6
    x = swept(x, cutoff * (1 + 2.5 * np.exp(-t / 0.05)))
    x += np.sin(2 * np.pi * f / 2 * t) * 0.9 + np.sin(2 * np.pi * f * t) * 0.3
    x *= envelope(n, 0.004, None, 1, length, 0.03)
    return tail_off(loud(ramp(x, 2)), 10)


# --- Drums and effects ------------------------------------------------------------------------


def kick(rng, big=False):
    t = seconds(0.9 if big else 0.55)
    f = 46 + 120 * np.exp(-t / 0.028) + 300 * np.exp(-t / 0.0035)
    phase = 2 * np.pi * np.cumsum(f) / SR
    body = np.sin(phase) * np.exp(-t / (0.42 if big else 0.26))
    body = np.tanh(1.8 * body) / np.tanh(1.8)
    click = np.zeros_like(t)
    n = int(0.003 * SR)
    click[:n] = noise(rng, n, band(1500, 8000)) * np.hanning(n)
    return tail_off(loud(ramp(body + 0.25 * click / np.max(np.abs(click)), 0.2)), 15)


def clap(rng):
    """Three hands just apart, a bit of snare under them, and their tail."""
    t = seconds(0.4)
    x = np.zeros_like(t)
    for k, at in enumerate((0.0, 0.009, 0.019)):
        i = int(at * SR)
        n = len(t) - i
        x[i:] += noise(rng, n, band(900, 3200)) * np.exp(-t[:n] / (0.007 if k < 2 else 0.11))
    body = np.sin(2 * np.pi * 195 * t) * np.exp(-t / 0.05) * 0.5
    snap = noise(rng, len(t), butter(2, 5000, btype="highpass", fs=SR, output="sos")) * np.exp(-t / 0.04) * 0.4
    return tail_off(loud(ramp(x / np.std(x[:2000]) * 0.5 + body + snap, 0.3)), 20)


def snare(rng):
    t = seconds(0.3)
    body = (np.sin(2 * np.pi * 185 * t) + 0.6 * np.sin(2 * np.pi * 330 * t)) * np.exp(-t / 0.05)
    wires = noise(rng, len(t), band(1200, 9000)) * np.exp(-t / 0.09)
    return tail_off(loud(ramp(0.6 * body + wires / np.std(wires[:1000]) * 0.5, 0.3)), 20)


def hat(rng, open_=False):
    t = seconds(0.45 if open_ else 0.09)
    metal = sum(np.sign(np.sin(2 * np.pi * f * t + rng.random() * 6)) for f in (317, 461, 587, 769, 853, 1213))
    x = sosfilt(band(6500, 11500, 3), metal * 0.15 + rng.standard_normal(len(t)))
    x *= np.exp(-t / (0.16 if open_ else 0.022))
    return tail_off(loud(ramp(x, 0.3)), 10)


def shaker(rng):
    t = seconds(0.08)
    x = noise(rng, len(t), band(4000, 11000)) * np.where(t < 0.008, t / 0.008, np.exp(-(t - 0.008) / 0.018))
    return tail_off(loud(x), 10)


def tom(rng, f):
    t = seconds(0.5)
    phase = 2 * np.pi * np.cumsum(f * (1 + 0.6 * np.exp(-t / 0.03))) / SR
    x = np.sin(phase) * np.exp(-t / 0.2) + 0.2 * noise(rng, len(t), band(800, 5000)) * np.exp(-t / 0.01)
    return tail_off(loud(ramp(x, 0.3)), 20)


def crash(rng):
    t = seconds(2.2)
    metal = sum(np.sin(2 * np.pi * f * t + rng.random() * 6) for f in (3170, 4410, 5260, 6790, 8120, 9450))
    x = sosfilt(band(3000, 12000), rng.standard_normal(len(t)) + 0.3 * metal) * np.exp(-t / 0.65)
    x = np.stack([x, sosfilt(band(3000, 12000), rng.standard_normal(len(t))) * np.exp(-t / 0.65)], axis=1)
    return tail_off(loud(ramp(x, 0.5)), 50)


def impact(rng, length=1.6):
    """A deep boom with a dusty thump on top: the card's slam."""
    t = seconds(length)
    f = 38 + 40 * np.exp(-t / 0.08)
    boom = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / (length / 3))
    thump = noise(rng, len(t), band(60, 900)) * np.exp(-t / 0.05)
    air = noise(rng, len(t), band(400, 4000)) * np.exp(-t / 0.35) * 0.15
    return tail_off(loud(ramp(boom + 0.4 * thump / np.std(thump[:2000]) + air, 0.5)), 50)


def riser(rng, length):
    """White noise through a rising band, and a sine sliding up an octave and a half."""
    n = int(length * SR)
    t = np.arange(n) / SR
    u = t / length
    x = swept(rng.standard_normal(n), 400 * (8000 / 400) ** u, "bandpass")
    x = x / np.std(x) * 0.5 + 0.25 * np.sin(2 * np.pi * np.cumsum(220 * 2 ** (1.5 * u)) / SR)
    x *= u**2
    x = tail_off(ramp(x, 5), 6)
    return x * db(-18) / np.sqrt(np.mean(x[-SR // 10 :] ** 2))  # ends 6 dB under a kick


def reverse_crash(rng, length):
    c = crash(rng)[: int(length * SR)][::-1].copy()
    return tail_off(c, 6)


def downlifter(rng, length=1.7):
    n = int(length * SR)
    t = np.arange(n) / SR
    u = t / length
    x = swept(rng.standard_normal(n), 7000 * (250 / 7000) ** u, "bandpass") * (1 - u) ** 2
    return ramp(x / np.std(x[: SR // 10]), 20) * db(-14)


def room(rng, rt60, pre=0.015, lp=6000):
    """Decaying noise, different left and right, a pre-delay, low-passed."""
    t = seconds(rt60 * 1.3)
    env = np.exp(-6.908 * t / rt60)
    channels = []
    for _ in range(2):
        ir = ramp(highpass(lowpass(rng.standard_normal(len(t)), lp), 150) * env, 2)
        ir = np.concatenate([np.zeros(int(pre * SR)), tail_off(ir, 50)])
        channels.append(ir / np.sqrt(np.sum(ir**2)))
    return np.stack(channels, axis=1)


# --- Harmony and melody -----------------------------------------------------------------------

CHORDS = {  # pitch classes, root first, as written
    "Dm9": "D F A C E", "Bbmaj7": "Bb D F A", "Gm9": "G Bb D F A", "A7sus4": "A D E G", "Fmaj7": "F A C E",
    "C": "C E G D", "Csus4": "C F G D", "Fadd9": "F A C G",
}


def pcs(symbol):
    return [midi(n + "0") % 12 for n in CHORDS[symbol].split()]


def voicing(symbol, lo, hi, previous=None, size=4):
    """Close voice leading: of the chord's tones, the set in range nearest the last chord."""
    tones = pcs(symbol)[1:] if len(pcs(symbol)) > size else pcs(symbol)
    tones = tones[:size]
    options = [[m for m in range(lo, hi + 1) if m % 12 == pc] for pc in tones]
    best, cost = None, None
    for combo in np.array(np.meshgrid(*options)).T.reshape(-1, len(tones)):
        notes = sorted(int(n) for n in combo)
        if notes[-1] - notes[0] > 16:
            continue
        c = sum(abs(a - b) for a, b in zip(notes, previous)) if previous else abs(np.mean(notes) - (lo + hi) / 2)
        if cost is None or c < cost:
            best, cost = notes, c
    return best


# The motif: sixteenths in the bar and their lengths, the same tresillo every time;
# the notes follow the chord.
RHYTHM = [(0, 3), (3, 3), (6, 2), (8, 2), (10, 3), (13, 3)]
MOTIF = {
    "Dm9": "D5 A5 F5 E5 D5 A4", "Bbmaj7": "D5 A5 F5 E5 D5 Bb4", "Gm9": "D5 A5 F5 E5 D5 G4",
    "A7sus4": "E5 A5 G5 E5 D5 A4", "Fmaj7": "C5 A5 F5 E5 C5 A4", "C": "E5 G5 E5 D5 C5 G4",
    "Csus4": "F5 G5 F5 D5 C5 G4",
}
ANSWER = {  # the motif's answer, a third higher: block 8
    "Dm9": "F5 C6 A5 G5 F5 C5", "Bbmaj7": "F5 D6 A5 G5 F5 D5", "Fmaj7": "A5 C6 A5 G5 F5 C5",
    "C": "G5 C6 G5 E5 D5 C5",
}
COUNTER = {  # long notes under the motif: blocks 6 and 9
    "Dm9": "A4", "Bbmaj7": "F4", "Gm9": "Bb4", "A7sus4": "G4", "Csus4": "F4", "C": "E4",
}
ROOTS = {"Dm9": "D2", "Bbmaj7": "Bb1", "Gm9": "G1", "A7sus4": "A1", "Fmaj7": "F2", "C": "C2", "Csus4": "C2",
         "Fadd9": "F1"}

BLOCKS = {
    0: "Dm9 Dm9",
    1: "Dm9 Bbmaj7 Gm9 A7sus4",
    2: "Dm9 Bbmaj7 Gm9 A7sus4",
    3: "Gm9 Bbmaj7 Dm9 A7sus4",
    4: "Dm9 Bbmaj7 Fmaj7 C",
    5: "Bbmaj7 C Dm9 A7sus4",
    6: "Dm9 Bbmaj7 Gm9 A7sus4",
    7: "Dm9 Bbmaj7 Fmaj7 C",
    8: "Dm9 Bbmaj7 Fmaj7 C",
    9: "Gm9 Bbmaj7 Csus4 C",
}


# --- Score ------------------------------------------------------------------------------------


def at(t16):
    """The sample of a sixteenth counted from bar 0's downbeat (negative in the pickup)."""
    return FIRST_BAR + t16 * SIXTEENTH


def accent_t16(a):
    """Card a (1-10) slams on bar 4a-2."""
    return (4 * a - 2) * 16


S16 = SIXTEENTH / SR
BAR = 16 * S16


class Score:
    def __init__(self):
        self.events = []  # (sample, maker, args, dB, pan, bus)
        self.kicks = []  # (sample, depth) for the sidechain
        self.gates = []  # (start, end): the band muted (stutters, the gap before the drop, the breath before a card)
        self.drum_gates = []  # (start, end): the drums muted, the last sixteenth before a card
        self.sweeps = []  # (start, end, from Hz, to Hz): the band's filter closing before the drop

    def add(self, t16_or_sample, maker, args=(), level=0.0, pan=0.0, bus="drums", sample=False):
        s = int(t16_or_sample) if sample else at(t16_or_sample)
        self.events.append((s, maker, tuple(args), level, pan, bus))


def compose(rng):
    sc = Score()
    prev = {"pad": None, "saw": None}

    def kick_at(t16, level=0.0, big=False, duck=0.5):
        sc.add(t16, "kick", (big,), level, 0, "drums")
        sc.kicks.append((at(t16), duck))

    # --- Pickup and intro (block 0): the motif from sample 0 ----------------------------------
    for i, (t, name, length) in enumerate([(-8, "D5", 3), (-5, "A5", 3), (-2, "F5", 2)]):
        sc.add(t, "pluck", (midi(name), length * S16, 0.55), -6 + 1.5 * i, 0.1, "pluck")
    for bar, symbol in enumerate(BLOCKS[0].split()):
        b = bar * 16
        motif(sc, b, symbol, 0.45 + 0.15 * bar, -7)
        for k in range(16):
            sc.add(b + k, "hat", (False,), -17 + (2 if k % 4 == 2 else 0) + bar, 0.3, "drums")
        for k in (2, 6, 10, 14):
            sc.add(b + k, "bass", (midi(ROOTS[symbol]) + 12, 1.6 * S16, 350 + 250 * bar), -10, 0, "music")
        prev["pad"] = voicing(symbol, 57, 76, prev["pad"])
        sc.add(b, "pad", (tuple(prev["pad"]), BAR, 900 + 600 * bar), -16, 0, "music")
    sc.add(at(32) - int(2 * BAR * SR), "riser", (2 * BAR - 0.03,), -9, 0, "fx", sample=True)

    # --- Blocks 1 to 9 -------------------------------------------------------------------
    for a in range(1, 10):
        start = accent_t16(a)
        chords = BLOCKS[a].split()
        drop = a >= 7
        accent(sc, a, start, chords[0], prev, kick_at)
        for bar, symbol in enumerate(chords):
            b = start + bar * 16
            last_bar = bar == 3
            building = a == 6 and bar >= 2  # the build before the drop

            # Kick: four to the floor, but not in the build's last two bars.
            if not building:
                for k in (0, 4, 8, 12):
                    if b + k == start:
                        continue  # the accent has its own kick
                    if a == 5 and last_bar and k == 12:
                        continue  # the stutter
                    kick_at(b + k, -1.5 if a == 1 else 0, duck=0.45 if not drop else 0.65)
                if drop or a >= 4:
                    if bar % 2 == 1 and not last_bar:
                        kick_at(b + 14, -9, duck=0.2)  # a ghost kick, every other bar

            # Claps on 2 and 4 (block 1 from bar 3), and the answer two beats after the accent.
            if not building and (a > 1 or bar >= 2):
                for k in (4, 12):
                    if not (a in (5, 8) and last_bar and k == 12):
                        sc.add(b + k, "clap", (), -5, 0, "drums")
            # Hats: 16ths, open on the offbeats from block 2.
            if not building:
                for k in range(16):
                    if a in (5, 8) and last_bar and k >= 12:
                        continue
                    accent_16 = 2 if k % 4 == 2 else 0
                    sc.add(b + k, "hat", (False,), -15 + accent_16 + (1 if drop else 0), 0.3 - 0.6 * (k % 2), "drums")
                if a >= 2:
                    for k in (2, 6, 10, 14):
                        if not (a in (5, 8) and last_bar and k >= 12):
                            sc.add(b + k, "hat", (True,), -15 if not drop else -13, -0.25, "drums")
            if a in (5, 9) and not building:
                for k in range(16):
                    sc.add(b + k, "shaker", (), -21 + (2 if k % 2 else 0), 0.55, "drums")

            # Bass.
            root = midi(ROOTS[symbol])
            if building:
                pass
            elif a == 5:
                for k in range(16):  # rolling sixteenths
                    if not (last_bar and k >= 12):
                        sc.add(b + k, "bass", (root + (12 if k % 4 == 3 else 0), 0.9 * S16, 800), -7 + (1 if k % 4 == 2 else 0), 0, "music")
            else:
                for k in (2, 6, 10, 14):  # the offbeat pump
                    if a == 8 and last_bar and k >= 12:
                        continue
                    sc.add(b + k, "bass", (root + 12, 1.7 * S16, 900 if drop else 650), -6, 0, "music")
                if drop:
                    sc.add(b, "bass", (root, BAR - 0.05, 300), -8, 0, "music")  # the sub under it all

            # Pads and supersaws.
            prev["pad"] = voicing(symbol, 57, 76, prev["pad"])
            if building:
                pass
            elif drop:
                prev["saw"] = voicing(symbol, 62, 81, prev["saw"])
                sc.add(b, "chord", (tuple(prev["saw"]), BAR - 0.03, 6500), -12, 0, "music")
                sc.add(b, "pad", (tuple(n - 12 for n in prev["pad"]), BAR, 2200), -16, 0, "music")
            else:
                sc.add(b, "pad", (tuple(prev["pad"]), BAR, 1500 + 300 * a), -15, 0, "music")
            if a in (3, 4, 5, 6) and not building:
                stabs = (2, 6, 10, 14) if a != 5 else (3, 6, 10)
                sv = voicing(symbol, 62, 79, None)
                for k in stabs:
                    if not (last_bar and k >= 12 and a == 5):
                        sc.add(b + k, "stab", (tuple(sv), 1.2 * S16, 4000), -15, 0, "music")

            # The motif.
            if not building:
                if a == 8:
                    motif(sc, b, symbol, 1.0, -5, ANSWER)
                elif not (a in (5,) and last_bar):
                    motif(sc, b, symbol, 0.7 + 0.05 * a if not drop else 1.0, -6 if not drop else -5)
                if a == 4 or drop:
                    motif(sc, b, symbol, 0.9, -12, octave=12)  # doubled an octave up
                if drop and a != 8:
                    for (k, length), name in zip(RHYTHM, MOTIF[symbol].split()):
                        sc.add(b + k, "lead", (midi(name), length * S16 - 0.02), -9, 0, "music")
            if (a in (4, 8, 9) or a == 7 and bar >= 2) and not building:
                tones = voicing(symbol, 69, 86, None)
                for k in range(16):
                    if not (a == 8 and last_bar and k >= 12):
                        sc.add(b + k, "pluck", (tones[[0, 1, 2, 3, 2, 1, 3, 2][k % 8] % len(tones)], 0.8 * S16, 0.5),
                               -17, 0.45 if k % 2 else -0.45, "pluck")
            if a in (6, 9) and not building:
                sc.add(b, "pluck", (midi(COUNTER[symbol]), BAR - 0.05, 0.25), -12, -0.3, "pluck")

        # --- The way into the next card -------------------------------------------------
        nxt = start + 64
        fills(sc, a, start, nxt, rng)
    final(sc, accent_t16(10), prev, kick_at)
    for a in range(1, 11):
        hit = at(accent_t16(a))
        sc.gates.append((hit - 2 * SIXTEENTH, hit - 1))
        sc.drum_gates.append((hit - SIXTEENTH, hit - 1))
    return sc


def motif(sc, b, symbol, bright, level, table=MOTIF, octave=0):
    for (k, length), name in zip(RHYTHM, table[symbol].split()):
        sc.add(b + k, "pluck", (midi(name) + octave, length * S16 - 0.01, bright), level - (2 if k in (6, 8) else 0),
               0.15 if octave else 0.0, "pluck")


def accent(sc, a, t, symbol, prev, kick_at):
    """The card's slam, and its answer two beats later."""
    big = a in (1, 5, 7)
    kick_at(t, 1.0 if a == 7 else 0.0, big=True, duck=0.85)
    sc.add(t, "impact", (2.2 if a == 7 else 1.6 if big else 1.0,), 0 if a == 7 else -2 if big else -5, 0, "hits")
    sc.add(t, "crash", (), -13 if a == 1 else -10 if a != 7 else -7, 0, "hits")
    if a >= 3:
        sv = voicing(symbol, 62, 79, None)
        sc.add(t, "stab", (tuple(sv), 3 * S16, 6000), -7 if a != 7 else -5, 0, "hits")
    if a == 7:
        sc.add(t, "downlifter", (), -6, 0, "fx")
    # The answer: a clap and a short stab on beat 3 of the card's bar.
    sc.add(t + 8, "clap", (), -3, 0, "hits")
    sv = voicing(symbol, 64, 81, None)
    sc.add(t + 8, "stab", (tuple(sv), 1.5 * S16, 3000), -13, 0.1, "hits")


def fills(sc, a, start, nxt, rng):
    last = nxt - 16
    if a == 1:  # snare eighths on the last two beats
        for i, k in enumerate((8, 10, 12, 14)):
            sc.add(last + k, "snare", (), -14 + 2 * i, 0, "drums")
    elif a == 2:  # a noise riser over the last bar
        sc.add(at(nxt) - int(BAR * SR), "riser", (BAR - 0.02,), -6, 0, "fx", sample=True)
    elif a == 3:  # toms, falling
        for i, (k, f) in enumerate(((8, 220), (10, 185), (11, 160), (12, 140), (14, 120), (15, 100))):
            sc.add(last + k, "tom", (f,), -10 + i, 0.4 - 0.15 * i, "drums")
    elif a == 4:  # a reverse cymbal and a run of snares
        sc.add(at(nxt) - int(BAR * SR), "revcrash", (BAR,), -10, 0, "fx", sample=True)
        for i, k in enumerate(range(12, 16)):
            sc.add(last + k, "snare", (), -12 + 1.5 * i, 0, "drums")
    elif a in (5, 8):  # the stutter: the last beat stops but for four claps
        sc.gates.append((at(last + 12), at(nxt) - 1))
        for i, k in enumerate(range(12, 16)):
            sc.add(last + k, "clap", (), -12 + 2 * i, 0, "drums")
    elif a == 6:  # the build: filter closes over bars 1-2, snare roll and riser, one beat of silence
        sc.sweeps.append((at(start), at(start + 32), 14000, 900))
        roll = [32 + 4 * i for i in range(4)] + [48 + 2 * i for i in range(4)] + [56 + i for i in range(4)]
        for i, k in enumerate(roll):
            sc.add(start + k, "snare", (), -18 + 0.9 * i, 0, "drums")
        sc.add(at(start + 32), "riser", (2 * BAR - 4 * S16 - 0.01,), -2, 0, "fx", sample=True)
        sc.gates.append((at(nxt - 4), at(nxt) - 1))
        for k in (32, 40):
            sc.add(start + k, "kick", (False,), -4, 0, "drums")
    elif a == 7:  # crash-and-snare fill
        for i, k in enumerate((8, 10, 12, 13, 14, 15)):
            sc.add(last + k, "snare", (), -12 + 1.2 * i, 0, "drums")
    elif a == 9:  # the final build: a snare roll and a riser
        roll = [0, 4, 8, 12] + [16, 18, 20, 22] + list(range(24, 32))  # quarters, eighths, sixteenths
        for i, k in enumerate(roll):
            sc.add(nxt - 32 + k, "snare", (), -20 + 0.6 * i, 0, "drums")
        sc.add(at(nxt) - int(2 * BAR * SR), "riser", (2 * BAR - 0.02,), -3, 0, "fx", sample=True)


def final(sc, t, prev, kick_at):
    """The end card: the whole band on F, then nothing but its rooms."""
    kick_at(t, 1.5, big=True, duck=0.0)
    sc.add(t, "impact", (3.0,), 0, 0, "end")
    sc.add(t, "crash", (), -6, 0, "end")
    chord = voicing("Fadd9", 60, 81, prev["saw"])
    sc.add(t, "chord", (tuple(chord), 2.2, 5000), -8, 0, "end")
    sc.add(t, "pad", (tuple(voicing("Fadd9", 53, 72, prev["pad"])), 2.6, 2000), -12, 0, "end")
    sc.add(t, "bass", (midi("F1"), 2.4, 400), -4, 0, "end")
    sc.add(t, "pluck", (midi("A5"), 1.0, 1.0), -4, 0, "endpluck")
    sc.add(t, "pluck", (midi("F5"), 1.0, 1.0), -8, 0, "endpluck")
    sc.add(t, "clap", (), -2, 0, "end")
    sc.add(t + 8, "pluck", (midi("C6"), 0.5, 0.6), -14, 0.3, "endpluck")  # the answer, softly


# --- Rendering --------------------------------------------------------------------------------


class Maker:
    def __init__(self, rng):
        self.rng, self.cache = rng, {}

    def take(self, name, args, takes=4):
        key = (name, args, int(self.rng.integers(takes)))
        if key not in self.cache:
            self.cache[key] = self.make(name, args)
        return self.cache[key]

    def make(self, name, args):
        r = self.rng
        if name in ("chord", "pad", "stab"):
            notes, length, cutoff = args
            maker = pad if name == "pad" else supersaw
            kw = {} if name == "pad" else {"release": 0.05 if name == "stab" else 0.3}
            out = sum(maker(r, m, length, cutoff=cutoff, **kw) for m in notes)
            return out / np.sqrt(len(notes))
        if name == "lead":
            return supersaw(r, args[0], args[1], cutoff=7000, release=0.08, voices=5, spread=0.3)
        return {
            "kick": lambda: kick(r, *args), "clap": lambda: clap(r), "snare": lambda: snare(r), "hat": lambda: hat(r, *args),
            "shaker": lambda: shaker(r), "tom": lambda: tom(r, *args), "crash": lambda: crash(r),
            "impact": lambda: impact(r, *args), "riser": lambda: riser(r, *args), "revcrash": lambda: reverse_crash(r, *args),
            "downlifter": lambda: downlifter(r), "pluck": lambda: pluck(r, *args), "bass": lambda: bass(r, *args),
        }[name]()


def place(bus, at_, x, pan=0.0):
    if at_ >= len(bus):
        return
    end = min(len(bus), at_ + len(x))
    x = x[: end - at_]
    if x.ndim == 1:
        left, right = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
        bus[at_:end, 0] += x * left * np.sqrt(2)
        bus[at_:end, 1] += x * right * np.sqrt(2)
    else:
        bus[at_:end] += x


def sidechain(kicks):
    """The pump: every kick ducks the band and lets it swell back over 150 ms."""
    t = seconds(0.45)
    shape = np.minimum(1, t / 0.004) * np.exp(-t / 0.11)
    duck = np.zeros(LENGTH)
    for s, depth in kicks:
        end = min(LENGTH, s + len(shape))
        duck[s:end] = np.maximum(duck[s:end], depth * shape[: end - s])
    return 1 - duck


def delay(x, beats=0.75, feedback=0.38, repeats=6):
    """A ping-pong delay, a dotted eighth, each repeat darker."""
    d = int(beats * 4 * SIXTEENTH)
    out = np.zeros((len(x), 2))
    y = x.copy()
    for r in range(1, repeats + 1):
        y = lowpass(y, 5000 - 500 * r)
        s = d * r
        if s >= len(x):
            break
        out[s:, r % 2] += y[: len(x) - s] * feedback**r
    return out


def mix(rng):
    sc = compose(rng)
    maker = Maker(rng)
    buses = {name: np.zeros((LENGTH, 2)) for name in ("drums", "music", "pluck", "hits", "fx", "end", "endpluck")}
    for s, name, args, level, pan, bus in sc.events:
        place(buses[bus], s, maker.take(name, args) * db(level), pan)

    pump = sidechain(sc.kicks)[:, None]
    band_ = buses["music"] * pump + buses["pluck"] * (0.5 + 0.5 * pump)
    for s, e, f0, f1 in sc.sweeps:  # the band's filter closing before the drop
        n = e - s
        cut = f0 * (f1 / f0) ** (np.arange(n) / n)
        for c in range(2):
            band_[s:e, c] = swept(band_[s:e, c], cut)
        # stays closed until the drop
        for c in range(2):
            band_[e : at(accent_t16(7)), c] = lowpass(band_[e : at(accent_t16(7)), c], f1)
    def gate(x, s, e):
        n = e - s
        g = np.zeros(n)
        g[:240] = np.linspace(1, 0, 240)  # 5 ms out
        g[-48:] = np.linspace(0, 1, 48)  # and back in for the hit
        x[s:e] *= g[:, None]

    for s, e in sc.gates:
        gate(band_, s, e)
    for s, e in sc.drum_gates:
        gate(buses["drums"], s, e)

    hall = room(rng, 2.0, 0.02, 7000)
    drum_room = room(rng, 0.6, 0.008, 9000)
    end_hall = room(rng, 2.6, 0.02, 6000)
    wet = np.zeros((LENGTH, 2))

    def send(x, ir, level):
        mono = x.mean(axis=1)
        for c in range(2):
            wet[:, c] += oaconvolve(mono, ir[:, c])[:LENGTH] * db(level)

    send(buses["drums"], drum_room, -16)
    send(band_, hall, -15)
    band_ *= db(-2.5)  # the band sits under the cards
    send(buses["hits"] + buses["fx"], hall, -10)
    echoes = delay((buses["pluck"] + buses["endpluck"]).mean(axis=1))
    send(buses["end"] + buses["endpluck"], end_hall, -7)
    out = buses["drums"] + band_ + buses["hits"] + buses["fx"] + wet + echoes * db(-9)
    arc = np.zeros(LENGTH)
    for a, gain in enumerate([-3.5, -3, -2.2, -1.6, -1.0, -0.5, 0, 0.8, 0.8, 1.2]):  # block 0 (intro) to 9
        arc[max(0, at(accent_t16(a)) if a else 0) :] = gain
    arc = np.convolve(arc, np.ones(2400) / 2400, mode="same")  # 50 ms glides, hidden under the cards
    out = out * db(arc)[:, None] + buses["end"] + buses["endpluck"]
    out = highpass(out, 25)
    fade = int(FADE_SECONDS * SR)
    out[-fade:] *= np.cos(np.linspace(0, np.pi / 2, fade))[:, None] ** 2
    return out / np.max(np.abs(out)) * db(-3)


def hit_list(raw):
    """hits.json: the video's beats 1 to 10 of each block, the card's accent
    (11) and the answer two beats later (echo), each with its level as
    sounded: the RMS of its first 100 ms against the accents of blocks 1 to 9."""
    rms = lambda s: np.sqrt(np.mean(raw[s : s + SR // 10] ** 2))
    pas = np.mean([rms((PHRASE_FRAMES * (p - 1) + FRAMES_PER_BEAT * 10) * SPF) for p in range(1, PHRASES)])
    hits = []
    for p in range(1, PHRASES + 1):
        start = PHRASE_FRAMES * (p - 1)
        for n, beat in [(n, n) for n in range(1, 12)] + [("echo", 13)]:
            frame = start + FRAMES_PER_BEAT * (beat - 1)
            hits.append({"phrase": p, "hit": n, "frame": frame, "time": round(frame / FPS, 6),
                         "level_db": round(20 * np.log10(rms(frame * SPF) / pas), 2)})
    return hits


# --- Mastering ----------------------------------------------------------------------------


def measure(path):
    err = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-i", str(path), "-af",
                          f"loudnorm=I={TARGET_I}:TP={TARGET_TP}:LRA=20:print_format=json", "-f", "null", "-"],
                         capture_output=True, text=True, check=True).stderr
    return json.loads(err[err.rindex("{") : err.rindex("}") + 1])


def true_peak_envelope(x):
    """The 4x oversampled peak of every sample, both channels."""
    up = np.abs(resample_poly(x, 4, 1, axis=0)).max(axis=1)
    return up[: len(x) * 4].reshape(-1, 4).max(axis=1)


def saturate(x, knee, ceiling):
    """A soft knee, run at 4x so its harmonics do not fold back: untouched
    under the knee, bending smoothly towards the ceiling over it."""
    up = resample_poly(x, 4, 1, axis=0)
    over = np.abs(up) > knee
    span = ceiling - knee
    up[over] = np.sign(up[over]) * (knee + span * np.tanh((np.abs(up[over]) - knee) / span))
    return resample_poly(up, 1, 4, axis=0)[: len(x)]


def limit(x, ceiling, release_ms=30):
    """A lookahead peak limiter on 1 ms blocks: the gain is down before a peak
    arrives, so attacks keep their time, and it lets go quickly."""
    block = SR // 1000
    n = -(-len(x) // block)
    peaks = np.pad(true_peak_envelope(x), (0, n * block - len(x))).reshape(n, block).max(axis=1)
    want = np.minimum(1.0, ceiling / np.maximum(peaks, 1e-12))
    g = np.empty(n)
    held, release = 1.0, np.exp(-1 / release_ms)
    for i in range(n):
        held = min(want[i], 1 - (1 - held) * release)
        g[i] = held
    # Min over +-3 blocks, then a 5-block average: smooth, and never above
    # what any block in reach needs.
    padded = np.pad(g, 3, constant_values=1.0)
    g = np.min(np.stack([padded[k : k + n] for k in range(7)]), axis=0)
    g = np.convolve(np.pad(g, 2, mode="edge"), np.ones(5) / 5, mode="valid")
    centres = np.arange(n) * block + block / 2
    gain = np.interp(np.arange(len(x)), centres, g)
    print(f"  limiter: deepest gain reduction {-20 * np.log10(g.min()):.1f} dB")
    return x * gain[:, None]


def master(raw, out):
    first = measure(raw)
    chain = (f"loudnorm=I={TARGET_I}:TP={TARGET_TP}:LRA=20:measured_I={first['input_i']}:measured_TP={first['input_tp']}"
             f":measured_LRA={first['input_lra']}:measured_thresh={first['input_thresh']}:offset={first['target_offset']}"
             f":linear=true:print_format=json,aresample={SR}")
    err = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-y", "-i", str(raw), "-af", chain,
                          "-c:a", "pcm_s24le", str(out)], capture_output=True, text=True, check=True).stderr
    applied = json.loads(err[err.rindex("{") : err.rindex("}") + 1])
    print(f"loudnorm pass 1: I {first['input_i']} LUFS, TP {first['input_tp']} dBTP, "
          f"pass 2 mode: {applied['normalization_type']}")
    if applied["normalization_type"] == "linear":
        x, _ = sf.read(out)
    else:
        # loudnorm gave up on linear (the PA peaks would pass the ceiling) and
        # went dynamic, which pumps and smears attacks. Instead: plain gain, a
        # soft knee that rounds the PA's stacked surdo peaks (they carry little
        # loudness), and a fast lookahead limiter as the true-peak guard.
        x, _ = sf.read(raw)
        gain_db = TARGET_I - float(first["input_i"])
        for _ in range(8):
            used = gain_db
            y = x * db(used)
            before = 20 * np.log10(np.abs(y).max())
            y = saturate(y, db(KNEE), db(TARGET_TP - 0.2))
            print(f"  gain {used:+.2f} dB: peak {before:+.1f} dBFS into the knee, "
                  f"{20 * np.log10(np.abs(y).max()):+.1f} out")
            y = limit(y, db(TARGET_TP - 0.4))
            sf.write(out, y, SR, subtype="PCM_24")
            got = float(measure(out)["input_i"])
            if abs(got - TARGET_I) < 0.05:
                break
            gain_db += TARGET_I - got
        x = y
        print(f"fallback: gain {used:+.2f} dB, knee at {KNEE} dBFS, lookahead limiter")
    x = x[:LENGTH] if len(x) >= LENGTH else np.pad(x, ((0, LENGTH - len(x)), (0, 0)))
    sf.write(out, x, SR, subtype="PCM_24")
    final = measure(out)
    print(f"mastered: I {final['input_i']} LUFS, TP {final['input_tp']} dBTP, LRA {final['input_lra']} LU")
    return x, final


# --- Checks -------------------------------------------------------------------------------


def onset_check(x, hits):
    """An onset envelope (2 ms RMS, centred, every sample); a card's hit
    starts where the envelope first passes a fifth (-14 dB) of the peak near
    its frame; the band is out for an eighth before it."""
    window = int(0.002 * SR)
    power = np.cumsum(np.concatenate([[0.0], (x**2).mean(axis=1)]))
    env = np.concatenate([np.zeros(window // 2), np.sqrt(np.maximum(power[window:] - power[:-window], 0) / window)])

    mono = np.abs(x).max(axis=1)
    first = np.argmax(mono > db(-40) * mono[: int(0.05 * SR)].max())
    print(f"phrase 1 hit 1: first sample within 40 dB of its peak: {first}")
    worst = 0.0
    for h in hits:
        if h["hit"] != 11:
            continue
        nominal = h["frame"] * SPF / SR
        lo, hi = int((nominal - 0.04) * SR), int((nominal + 0.1) * SR)
        seg = env[lo:hi]
        at = (lo + np.argmax(seg > 0.2 * seg.max())) / SR
        lead = env[int((nominal - 0.005) * SR)]
        offset = (at - nominal) * 1000
        worst = max(worst, abs(offset))
        if h["phrase"] in (1, 5, 10):
            print(f"PA phrase {h['phrase']:2d}: frame {h['frame']} = {nominal:.4f} s, attack at {at:.4f} s ({offset:+.2f} ms); "
                  f"5 ms before it the sound is {20 * np.log10(lead / seg.max()):.1f} dB under the hit")
    print(f"worst PA offset across all ten: {worst:.2f} ms (one frame is 16.7 ms)")
    return first, worst


def spectrum_check(x):
    dc = x.mean(axis=0)
    spec = np.abs(np.fft.rfft(x.mean(axis=1)))
    freqs = np.fft.rfftfreq(len(x), 1 / SR)
    total = np.sum(spec**2)
    share = lambda lo, hi: 10 * np.log10(np.sum(spec[(freqs >= lo) & (freqs < hi)] ** 2) / total + 1e-20)
    print(f"DC L {dc[0]:+.2e} R {dc[1]:+.2e}; energy below 30 Hz {share(0, 30):.1f} dB, 30-200 Hz {share(30, 200):.1f} dB, "
          f"200-2k {share(200, 2000):.1f} dB, 2-8 kHz {share(2000, 8000):.1f} dB, above 16 kHz {share(16000, 24000):.1f} dB; "
          f"sample peak {20 * np.log10(np.abs(x).max()):.2f} dBFS")


def png(path, x, hits):
    """The whole file above, phrase 1 below; hits marked on the zoom. No plotting library."""
    w, h = 1600, 760
    img = np.full((h, w, 3), 250, np.uint8)

    def panel(top, height, y, marks):
        mid = top + height // 2
        img[mid, :] = (210, 210, 210)
        for m in marks:
            img[top : top + height, int(m)] = (240, 170, 160)
        for c, seg in enumerate(np.array_split(y, w)):
            a, b = seg.min(), seg.max()
            y0, y1 = int(mid - b * height / 2), int(mid - a * height / 2)
            img[max(top, y0) : min(top + height, y1 + 1), c] = (30, 60, 130)

    mono = x.mean(axis=1)
    panel(20, 340, mono, [])
    zoom = mono[: PHRASE_FRAMES * SPF]
    marks = [hh["frame"] * SPF / len(zoom) * (w - 1) for hh in hits if hh["phrase"] == 1]
    panel(400, 340, zoom, marks)
    raw = b"".join(b"\x00" + row.tobytes() for row in img)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(SEED)
    raw = mix(rng)
    hits = hit_list(raw)
    sf.write(out / "drum-raw.wav", raw.astype(np.float32), SR, subtype="FLOAT")
    x, final = master(out / "drum-raw.wav", out / "drum.wav")
    (out / "hits.json").write_text(json.dumps({"fps": FPS, "frames": FRAMES, "frame_per_beat": FRAMES_PER_BEAT,
                                               "phrase_frames": PHRASE_FRAMES, "hits": hits}, indent=1) + "\n")
    first, worst = onset_check(x, hits)
    spectrum_check(x)
    png(out / "waveform.png", x, hits)
    print(f"{len(x)} samples, {len(x) / SR:.4f} s, {len(hits)} hits")
    print("phrase 1 levels, dB under the PA:", [h["level_db"] for h in hits if h["phrase"] == 1])
    problems = []
    if float(final["input_tp"]) > TARGET_TP:
        problems.append("true peak over the ceiling")
    if abs(float(final["input_i"]) - TARGET_I) > 0.5:
        problems.append("loudness off target")
    if first > 0.002 * SR:
        problems.append("the first hit is late")
    if worst > 1000 / FPS:
        problems.append("a PA is off its frame")
    if problems:
        sys.exit("; ".join(problems))


if __name__ == "__main__":
    main()
