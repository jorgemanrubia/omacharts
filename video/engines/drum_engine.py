#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "scipy", "soundfile"]
# ///
"""Synthesise the Trading video's music: a short, original samba tune.

    video/engines/drum_engine.py OUT_DIR

Writes OUT_DIR/drum.wav (48 kHz, 24-bit, stereo, mastered to -14 LUFS) and
OUT_DIR/hits.json, the frame, sounded time and level of every hit, which the
edit reads. Nothing is sampled: the drums are struck membranes (inharmonic
Bessel modes, a pitch drop, a stick or mallet transient), the cavaquinho,
the guitar and the bass are plucked strings built from their harmonics, the
flute is a breathy sine with vibrato, the agogô is metallic partials, the
ganzá and the snares are filtered noise. Deterministic from seed 7.

The grid is the score's (video/scripts/trading.md): 26 frames a beat at 60
fps (138.46 BPM), sixteen beats a phrase, ten phrases and a 170-frame tail.
A phrase is eight bars of 2/4. The tune's cadence lands on beat 11, the PA,
where a card slams; beat 13 is the echo, the card's second line; beats 14
to 16 are the breath and the pickup into the next phrase. The accented
downbeats of beats 1 to 10, every PA and every echo sound exactly on their
frames; everything else is played: a few ms early or late, a little louder
or softer, with samba's late second and fourth sixteenths.

Key of G major. Arrangement, one line a phrase (chords one a bar; * is the PA):

  1 intro      G6 G6 Cmaj7 Cmaj7 D7 *G6 Em7 D7         cavaquinho alone, sparse, ganzá; one tamborim from
                                                         beat 5; a lone tamborim run into a surdo-and-caixa stab
  2 intro      Em7 A7 Am7 Am7 D7 *Gmaj7 Em7 D7         + bass and a soft surdo pulse from beat 3, caixa ghosts
                                                         from 7; surdo stop into a stab; flute pickup D E F# A
  3 theme A    G6 E7 Am7 Am7 D7 *G6 Em7 D7             flute theme, guitar, marcação, caixa, teleco-teco,
                                                         agogô from 5; short repinique call; tutti on B
  4 theme A'   G6 E7 Am7 Cm6 D7 *G6 Em7 G7             second time, cadence up on D; terceira improvises;
                                                         caixa roll over beats 9-10; tutti; pickup G B D F
  5 lift B     Cmaj7 Bm7 Em7 A7 D7 *Gmaj7 Em7 G7       new strain up high, the full bateria, two caixas;
                                                         repinique calls, the band answers; tutti
  6 lift B'    Cmaj7 Bm7 Em7 A7 D7 *G6 Em7 Bm7→Em7     peak of the middle, flute to A6, three caixas; a
                                                         break of silence before the biggest tutti so far
  7 breakdown  Em7 Em7 Cmaj7 Cmaj7 Am7 *Bm7 Em7 Em7     drums drop out: surdos, bass, soft cavaquinho, low
                                                         flute; repinique and surdo trade calls; surdo-and-caixa stab
  8 breakdown  Cmaj7 Cmaj7 Bm7 Bm7 Am7 *D7sus4 D7 D7   barest: picked cavaquinho, ganzá, segunda pulse; flute
                                                         holds A over the suspended stab; a caixa roll rebuilds
  9 rebuild    G6 E7 Am7 Cm6 D7 *G6 Em7 G7             theme A' returns, layers come back in two-beat steps;
                                                         long repinique call; tutti; the apito calls the end
 10 climax     Cmaj7 Bm7 Em7 A7 D7 *G6/9 (rings)       strain B up to C7, everything, carreteiro tamborim,
                                                         caixa roll; the final chord on frame 4004 and its
                                                         ring-out to the end: no fade, the room dies away

Also leaves OUT_DIR/drum-raw.wav (the unmastered mix) and OUT_DIR/waveform.png
(the whole file, and phrase 1 up close), and prints the loudness and onset
checks.
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
SWING = 0.20  # the second and fourth sixteenths sit a fifth of a sixteenth late
SEED = 7


ROOM_SEND, PA_SEND = -14, -8
JITTER_SECONDS, JITTER_DB = 0.004, 0.8
FADE_SECONDS = 1.5
TARGET_I, TARGET_TP = -14.0, -1.0
KNEE = -7.0  # dBFS, where the master's soft knee starts

# Free circular membrane: the ratios of its first modes (Bessel zeros).
MODES = np.array([1.0, 1.594, 2.136, 2.296, 2.653, 2.918, 3.156, 3.501, 3.600, 3.652])



def db(x):
    return 10 ** (x / 20)


def seconds(n):
    return np.arange(int(n * SR)) / SR


def band(lo, hi, order=2):
    return butter(order, [lo, hi], btype="bandpass", fs=SR, output="sos")


def ramp(x, ms):
    """A raised-cosine onset, so nothing starts on a step."""
    n = max(1, int(ms * SR / 1000))
    x[:n] *= 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / n)
    return x


def tail_off(x, ms=20):
    n = int(ms * SR / 1000)
    x[-n:] *= np.linspace(1, 0, n)
    return x


def peak_to(x, level=1.0):
    return x * (level / np.max(np.abs(x)))


def noise(rng, n, sos):
    return sosfilt(sos, rng.standard_normal(n + 1024))[1024:]


# --- Instruments ----------------------------------------------------------------------------


def membrane(rng, f0, weights, tau, tilt, drop, drop_tau, length):
    """A struck head: its Bessel modes, each a little off from a perfect head,
    higher ones dying sooner, all bending down as the strike's extra tension
    lets go."""
    t = seconds(length)
    f0 *= 1 + rng.uniform(-0.012, 0.012)
    glide = t + drop * drop_tau * (1 - np.exp(-t / drop_tau))
    out = np.zeros_like(t)
    for r, w in zip(MODES, weights):
        r = r * (1 + rng.uniform(-0.006, 0.006))
        out += w * rng.uniform(0.75, 1.25) * np.sin(2 * np.pi * f0 * r * glide) * np.exp(-t * r**tilt / tau)
    return out


def transient(rng, length, lo, hi, tau, n_total):
    """A stick, hand or mallet: a few ms of band-passed noise."""
    x = np.zeros(n_total)
    n = int(length * SR)
    t = np.arange(n) / SR
    x[:n] = ramp(noise(rng, n, band(lo, hi)) * np.exp(-t / tau), 0.3)
    return x / np.max(np.abs(x))


def finish(x, ramp_ms=0.4):
    """Every stroke leaves here equally loud: the RMS of its first 60 ms at
    -12 dB, so a click-heavy stroke is not mistaken for a loud one."""
    x = tail_off(ramp(x, ramp_ms))
    return x * (db(-12) / np.sqrt(np.mean(x[: int(0.06 * SR)] ** 2)))


def surdo(rng, f0, open_=True):
    """The big bass drum, played with a felt mallet; open it rings, pressed
    with the other hand it thuds."""
    length = 1.8 if open_ else 0.5
    tau = 0.55 if open_ else 0.11
    head = membrane(rng, f0, [1, 0.42, 0.28, 0.2, 0.11, 0.08, 0.05, 0.03, 0.02, 0.02], tau, 0.9, 0.09, 0.05, length)
    n = len(head)
    mallet = transient(rng, 0.014, 110, 1100, 0.005, n) * 0.6
    shell = np.sin(2 * np.pi * f0 * 2.7 * seconds(length)) * np.exp(-seconds(length) / 0.06) * 0.08
    return finish(head + mallet + shell, 0.6)


def caixa(rng, kind):
    """The snare: a tight head, the wires on the bottom head, the stick."""
    level = {"accent": 1.0, "normal": 0.7, "ghost": 0.45}[kind]
    length = 0.45 if kind != "ghost" else 0.25
    t = seconds(length)
    head = membrane(rng, 255, [1, 0.6, 0.5, 0.35, 0.25, 0.2, 0.12, 0.1, 0.06, 0.05], 0.07, 0.6, 0.04, 0.012, length)
    wires_tau = {"accent": 0.10, "normal": 0.075, "ghost": 0.045}[kind]
    wires = noise(rng, len(t), band(1800, 8000, 3)) + 0.5 * noise(rng, len(t), band(600, 1800))
    wires = ramp(wires * np.exp(-t / wires_tau), 1.5) / np.std(wires[:480]) * 0.22
    bright = {"accent": (2500, 7500), "normal": (2000, 6000), "ghost": (1200, 4000)}[kind]
    stick = transient(rng, 0.004, *bright, 0.0012, len(t)) * 0.6 * level
    return finish(0.35 * head * level + 1.4 * wires * level + stick) * level


def repinique(rng, rim):
    """The high lead drum, stick in one hand; a rimshot cracks and rings the shell."""
    length = 0.6
    t = seconds(length)
    head = membrane(rng, 352, [1, 0.55, 0.45, 0.3, 0.22, 0.18, 0.12, 0.1, 0.07, 0.05], 0.2, 0.8, 0.05, 0.015, length)
    stick = transient(rng, 0.005, 2000, 8000, 0.0015, len(t)) * (0.7 if rim else 0.35)
    x = head + stick
    if rim:
        ring = sum(a * np.sin(2 * np.pi * f * t) * np.exp(-t / d)
                   for a, f, d in ((0.18, 1870, 0.05), (0.12, 2710, 0.035), (0.08, 3930, 0.025)))
        x += ramp(ring, 0.5)
    return finish(x)


def tamborim(rng, muted=False):
    """A small, very tight frame drum and a plastic stick: a sharp, short tok."""
    length = 0.25
    t = seconds(length)
    head = membrane(rng, 690, [1, 0.5, 0.35, 0.25, 0.15, 0.1, 0.07, 0.05, 0.03, 0.03],
                    0.022 if muted else 0.05, 0.7, 0.03, 0.006, length)
    stick = transient(rng, 0.003, 2500, 9000, 0.0009, len(t)) * 0.55
    return finish(head + stick)


def agogo(rng, high):
    """Two welded bells: inharmonic partials and the stick's tick."""
    length = 0.7
    t = seconds(length)
    f = (1040 if high else 770) * (1 + rng.uniform(-0.003, 0.003))
    x = np.zeros_like(t)
    for ratio, amp, decay in ((1, 1.0, 0.32), (2.32, 0.42, 0.14), (4.08, 0.22, 0.07), (6.31, 0.1, 0.04)):
        # a slow, small frequency wobble: hand on the bell
        x += amp * np.sin(2 * np.pi * f * ratio * t + 0.15 * np.sin(2 * np.pi * 5.3 * t)) * np.exp(-t / decay)
    stick = transient(rng, 0.002, 3000, 10000, 0.0007, len(t)) * 0.3
    return finish(x + stick, 0.3)


def ganza(rng, accent):
    """A metal shaker: the beads hit the far wall a few ms after the push."""
    length = 0.14
    t = seconds(length)
    grains = noise(rng, len(t), band(3500, 10000, 3)) + 0.4 * noise(rng, len(t), band(1800, 3500))
    rise = 0.004 if accent else 0.008
    env = np.where(t < rise, (t / rise) ** 2, np.exp(-(t - rise) / (0.022 if accent else 0.03)))
    x = grains * env * (1 + 0.35 * np.sign(noise(rng, len(t), band(200, 900))))  # rattle
    return finish(x, 1) * (1.0 if accent else 0.55)


def apito(rng, length):
    """The director's pea whistle: a trilling high tone over breath."""
    t = seconds(length)
    trill = np.sin(2 * np.pi * 36 * t)
    f = 3050 + 70 * trill
    tone = np.sin(2 * np.pi * np.cumsum(f) / SR) * (0.7 + 0.3 * trill)
    breath = noise(rng, len(t), band(1500, 7000)) * 0.12
    env = np.minimum(1, t / 0.015) * np.minimum(1, (length - t) / 0.035)
    return finish((tone + breath) * env, 1)


def room(rng, rt60):
    """Decaying noise, different left and right, 12 ms pre-delay, low-passed at 6 kHz."""
    t = seconds(rt60 * 1.3)
    env = np.exp(-6.908 * t / rt60)
    lowpass = butter(2, 6000, btype="lowpass", fs=SR, output="sos")
    highpass = butter(2, 90, btype="highpass", fs=SR, output="sos")  # keep the surdos tight
    pre = np.zeros(int(0.012 * SR))
    channels = []
    for _ in range(2):
        ir = ramp(sosfilt(highpass, sosfilt(lowpass, rng.standard_normal(len(t)))) * env, 2)
        ir = np.concatenate([pre, tail_off(ir, 50)])
        channels.append(ir / np.sqrt(np.sum(ir**2)))
    return np.stack(channels, axis=1)


class Kit:
    """A few takes of every stroke, so no two hits in a row are the same sound."""

    TAKES = 6

    def __init__(self, rng):
        make = {
            "primeira": lambda: surdo(rng, 52.0),
            "primeira-muted": lambda: surdo(rng, 52.0, open_=False),
            "segunda": lambda: surdo(rng, 61.5),
            "terceira": lambda: surdo(rng, 73.5),
            "terceira-muted": lambda: surdo(rng, 73.5, open_=False),
            "caixa-accent": lambda: caixa(rng, "accent"),
            "caixa-normal": lambda: caixa(rng, "normal"),
            "caixa-ghost": lambda: caixa(rng, "ghost"),
            "repinique": lambda: repinique(rng, rim=False),
            "repinique-rim": lambda: repinique(rng, rim=True),
            "tamborim": lambda: tamborim(rng),
            "tamborim-muted": lambda: tamborim(rng, muted=True),
            "agogo-high": lambda: agogo(rng, True),
            "agogo-low": lambda: agogo(rng, False),
            "ganza-accent": lambda: ganza(rng, True),
            "ganza": lambda: ganza(rng, False),
        }
        self.takes = {name: [f() for _ in range(self.TAKES)] for name, f in make.items()}
        self.rng = rng

    def __getitem__(self, name):
        return self.takes[name][self.rng.integers(self.TAKES)]


# --- Pitched instruments --------------------------------------------------------------------


def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)


def midi(name):
    """'F#5' -> 78."""
    pc = "C D EF G A B".index(name[0])
    rest = name[1:]
    while rest[0] in "#b":
        pc += 1 if rest[0] == "#" else -1
        rest = rest[1:]
    return 12 * (int(rest) + 1) + pc


STRINGS = {
    # tilt of the harmonics, stiffness, first decay, body resonances (Hz, gain), top harmonic, pick band
    "cavaco": dict(tilt=0.9, b=1.2e-4, tau=0.9, body=((470, 0.5), (980, 0.3)), top=9000, pick=(2000, 7000)),
    "violao": dict(tilt=1.3, b=2.5e-5, tau=1.6, body=((102, 0.6), (215, 0.4)), top=6000, pick=(1200, 5000)),
    "baixo": dict(tilt=1.8, b=1.5e-4, tau=1.1, body=((70, 0.3),), top=2500, pick=(80, 700)),
}


def string(rng, kind, m, dur):
    """A plucked string, from its harmonics: shaped by where it is plucked,
    a little stretched (a stiff string), the high ones dying sooner; the
    pick or the finger; the body's resonances; damped when the note ends."""
    p = STRINGS[kind]
    f0 = hz(m) * (1 + rng.uniform(-0.0012, 0.0012))
    length = dur + 0.09
    t = seconds(length)
    pos = rng.uniform(0.12, 0.22)
    tau = p["tau"] * (220 / max(f0, 60)) ** 0.3
    out = np.zeros_like(t)
    k = 1
    while k * f0 < p["top"] and k <= 30:
        fk = k * f0 * np.sqrt(1 + p["b"] * k * k)
        amp = abs(np.sin(np.pi * k * pos)) / k ** p["tilt"] * rng.uniform(0.85, 1.15)
        out += amp * np.sin(2 * np.pi * fk * t) * np.exp(-t * (1 + 0.004 * k * k * f0 / 200) / tau)
        k += 1
    out += transient(rng, 0.004, *p["pick"], 0.001, len(t)) * 0.08 * np.max(np.abs(out))
    for f, g in p["body"]:
        out += g * sosfilt(band(f * 0.92, f * 1.08), out)
    damp = np.where(t < dur, 1.0, np.exp(-(t - dur) / 0.018))
    return finish(out * damp, 0.5)


def flute(rng, m, dur):
    """A flute: a sine and a few soft harmonics, delayed vibrato, breath
    noise around the tone, a chiff at the start."""
    f0 = hz(m)
    length = dur + 0.15
    t = seconds(length)
    vib = 1 + 0.0055 * np.sin(2 * np.pi * 5.2 * t + rng.uniform(0, 6.28)) * np.clip((t - 0.2) / 0.3, 0, 1)
    phase = 2 * np.pi * np.cumsum(f0 * vib) / SR
    tone = np.sin(phase) + 0.2 * np.sin(2 * phase) + 0.07 * np.sin(3 * phase) + 0.03 * np.sin(4 * phase)
    wobble = 1 + 0.04 * peak_to(noise(rng, len(t), butter(1, 12, fs=SR, output="sos")))
    breath = noise(rng, len(t), band(min(f0 * 1.3, 9000), min(f0 * 5, 15000)))
    breath *= 0.05 / np.std(breath)
    chiff = transient(rng, 0.035, 1500, 6000, 0.012, len(t)) * 0.1
    attack = np.clip(t / 0.045, 0, 1) ** 1.5
    body = np.where(t < dur, 1 - 0.1 * t / max(dur, 0.1), (1 - 0.1) * np.exp(-(t - dur) / 0.05))
    return finish((tone * wobble + breath) * attack * body + chiff, 1)


QUALITIES = {"6": [0, 4, 7, 9], "maj7": [0, 4, 7, 11], "7": [0, 4, 7, 10], "m7": [0, 3, 7, 10],
             "m6": [0, 3, 7, 9], "7sus4": [0, 5, 7, 10], "6/9": [0, 4, 9, 14]}


def chord(symbol):
    """'Bm7' -> (root pitch class, [pitch classes])."""
    root = symbol[0] + (symbol[1] if len(symbol) > 1 and symbol[1] in "#b" else "")
    quality = symbol[len(root):]
    pc = midi(root + "0") % 12
    quality = {"m7": "m7", "m6": "m6"}.get(quality, quality)
    return pc, [(pc + i) % 12 for i in QUALITIES[quality]]


def voice(pcs, lo, hi, previous):
    """The voicing of these pitch classes between lo and hi nearest the last one."""
    options = [[m for m in range(lo, hi + 1) if m % 12 == pc] for pc in pcs]
    best, score_ = None, None
    for combo in np.array(np.meshgrid(*options)).T.reshape(-1, len(pcs)):
        notes = sorted(combo)
        if notes[-1] - notes[0] > 14 or len(set(notes)) < len(notes):
            continue
        s = sum(abs(a - b) for a, b in zip(notes, previous)) if previous else abs(np.mean(notes) - (lo + hi) / 2)
        if score_ is None or s < score_:
            best, score_ = notes, s
    return [int(n) for n in best]


class Strings:
    """Pitched notes, rendered once per pitch, length and take."""

    def __init__(self, rng):
        self.rng, self.cache = rng, {}

    def get(self, kind, m, dur):
        dur = round(dur / 0.02) * 0.02
        key = (kind, m, dur, int(self.rng.integers(3)))
        if key not in self.cache:
            self.cache[key] = flute(self.rng, m, dur) if kind == "flute" else string(self.rng, kind, m, dur)
        return self.cache[key]


# --- Score ----------------------------------------------------------------------------------

S16 = SIXTEENTH / SR  # seconds in a sixteenth

CHORDS = {
    1: "G6 G6 Cmaj7 Cmaj7 D7 G6 Em7 D7",
    2: "Em7 A7 Am7 Am7 D7 Gmaj7 Em7 D7",
    3: "G6 E7 Am7 Am7 D7 G6 Em7 D7",
    4: "G6 E7 Am7 Cm6 D7 G6 Em7 G7",
    5: "Cmaj7 Bm7 Em7 A7 D7 Gmaj7 Em7 G7",
    6: "Cmaj7 Bm7 Em7 A7 D7 G6 Bm7 Em7",
    7: "Em7 Em7 Cmaj7 Cmaj7 Am7 Bm7 Em7 Em7",
    8: "Cmaj7 Cmaj7 Bm7 Bm7 Am7 D7sus4 D7 D7",
    9: "G6 E7 Am7 Cm6 D7 G6 Em7 G7",
    10: "Cmaj7 Bm7 Em7 A7 D7 G6/9 G6/9 G6/9",
}

# The flute: sixteenth in the phrase, note, length in sixteenths. Beat 11 is sixteenth 40.
THEME_A = ("0 B5 3, 3 A5 1, 4 G5 2, 6 E5 2, 8 G#5 3, 11 B5 3, 14 D6 2, 16 C6 3, 19 B5 1, 20 A5 2, 22 E5 2, "
           "24 G5 2, 26 A5 2, 28 C6 3, 31 B5 1, 32 A5 2, 34 F#5 2, 36 A5 1, 37 C6 3, 40 B5 6, 48 E5 2, 50 G5 2, "
           "58 D5 1, 59 E5 1, 60 F#5 2, 62 A5 2")
THEME_A2 = ("0 B5 3, 3 A5 1, 4 G5 2, 6 E5 2, 8 G#5 3, 11 B5 3, 14 E6 2, 16 C6 3, 19 B5 1, 20 A5 2, 22 E5 2, "
            "24 G5 2, 26 A5 2, 28 C6 2, 30 Eb6 2, 32 D6 2, 34 C6 1, 35 A5 1, 36 F#5 2, 38 A5 2, 40 D6 6, "
            "48 B5 2, 50 G5 2, 56 G5 2, 58 B5 2, 60 D6 2, 62 F6 2")
MELODY = {
    1: "",
    2: "58 D5 1, 59 E5 1, 60 F#5 2, 62 A5 2",
    3: THEME_A,
    4: THEME_A2,
    5: ("0 E6 4, 4 D6 2, 6 C6 2, 8 D6 3, 11 B5 5, 16 G5 2, 18 B5 2, 20 D6 2, 22 E6 2, 24 C#6 3, 27 E6 1, "
        "28 G6 4, 32 F#6 2, 34 E6 2, 36 D6 2, 38 C6 2, 40 B5 8, 52 B5 2, 54 D6 2, 56 G6 2, 58 F6 2, 60 D6 2, 62 F6 2"),
    6: ("0 E6 4, 4 D6 2, 6 C6 2, 8 D6 3, 11 F#6 5, 16 G6 2, 18 E6 2, 20 B5 2, 22 E6 2, 24 C#6 2, 26 E6 2, "
        "28 A6 4, 32 A6 2, 34 G6 2, 36 F#6 2, 38 E6 1, 39 F#6 1, 40 G6 6, 48 F#6 2, 50 D6 2, 52 B5 4, 56 G5 4"),
    7: ("4 E5 4, 8 G5 2, 10 F#5 2, 12 E5 4, 20 G5 2, 22 E5 2, 24 D5 2, 26 E5 2, 28 G5 4, 32 A5 4, 36 C6 2, "
        "38 B5 2, 40 F#5 6, 52 E5 6"),
    8: "40 A5 8, 56 D5 2, 58 E5 2, 60 F#5 2, 62 A5 2",
    9: THEME_A2,
    10: ("0 E6 4, 4 D6 2, 6 C6 2, 8 D6 3, 11 B5 5, 16 G5 2, 18 B5 2, 20 D6 2, 22 E6 2, 24 C#6 2, 26 E6 2, "
         "28 A6 2, 30 G6 2, 32 F#6 2, 34 A6 2, 36 C7 2, 38 A6 2, 40 G6 24"),
}

# What plays, phrase by phrase. Beats are the first beat a layer plays (1-10).
PLAN = {
    1: dict(lift=-6, comp="intro", guitar=None, bass=None, ganza=(1, 1), tamborim=(5, "sparse", 1), caixa=None,
            surdo=None, terceira=None, agogo=None, fill="tamborim-run", pa="stab"),
    2: dict(lift=-6, comp="groove", guitar=None, bass=3, ganza=(1, 1), tamborim=(1, "sparse", 1),
            caixa=(7, "ghosts", 1), surdo=(3, "pulse"), terceira=None, agogo=None, fill="surdo-stop", pa="stab"),
    3: dict(lift=-3.5, comp="groove", guitar=1, bass=1, ganza=(1, 2), tamborim=(1, "teleco", 1),
            caixa=(1, "groove", 1), surdo=(1, "marcacao"), terceira=None, agogo=(5, "A"), fill="call-short", pa="tutti"),
    4: dict(lift=-2.5, comp="groove", guitar=1, bass=1, ganza=(1, 2), tamborim=(1, "teleco2", 2),
            caixa=(1, "groove", 2), surdo=(1, "marcacao"), terceira=3, agogo=(1, "B"), fill="roll", pa="tutti"),
    5: dict(lift=-1, comp="full", guitar=1, bass=1, ganza=(1, 2), tamborim=(1, "partido", 2),
            caixa=(1, "alt", 2), surdo=(1, "marcacao"), terceira=1, agogo=(1, "C"), fill="call-response", pa="tutti"),
    6: dict(lift=0, comp="full", guitar=1, bass=1, ganza=(1, 2), tamborim=(1, "teleco", 2),
            caixa=(1, "groove", 3), surdo=(1, "marcacao"), terceira=1, agogo=(1, "D"), fill="break", pa="tutti"),
    7: dict(lift=-7, comp="soft", guitar=None, bass=1, ganza=None, tamborim=None, caixa=None,
            surdo=(1, "marcacao"), terceira=None, agogo=None, fill="dialog", pa="stab"),
    8: dict(lift=-9, comp="picked", guitar=None, bass=5, ganza=(3, 1), tamborim=None, caixa=None,
            surdo=(1, "pulse"), terceira=None, agogo=None, fill="none", pa="stab", rebuild=True),
    9: dict(lift=-3, comp="groove", guitar=3, bass=1, ganza=(1, 2), tamborim=(5, "teleco2", 1),
            caixa=(3, "groove", "steps"), surdo=(1, "marcacao"), terceira=7, agogo=(7, "A"), fill="call-long",
            pa="tutti", apito=True),
    10: dict(lift=0.5, comp="full", guitar=1, bass=1, ganza=(1, 2), tamborim=(1, "partido", 2),
             caixa=(1, "alt", 3), surdo=(1, "marcacao"), terceira=1, agogo=(1, "B"), fill="final", pa="final"),
}

CRESCENDO = [-6, -5.5, -5, -4.5, -4, -3.5, -2.5, -2, -1, 0]
TAMBORIM_CELLS = {"sparse": "x...x...", "teleco": "x.xx.xx.", "teleco2": "x.xx.x.x", "partido": "xx.x.xx."}
CAIXA_CELLS = {"ghosts": "gg.gg.g.", "groove": "AgnAgnAg", "alt": "AggAgAgn"}
AGOGO_CELLS = {"A": "H.LH.L.L", "B": "H.HL.LH.", "C": "L.LHH.H.", "D": "HH.L.LL."}
TERCEIRA_CELLS = ["..o.m..o", "m..o.o..", ".o.om.oo", "o..m..o."]
# The cavaquinho's right hand: D/d down, U/u up (capital = accent), x a choked chop, . rest.
COMP_CELLS = {"intro": ["D...d.u.", "D.....u."], "groove": ["D.uD.u.u", ".uD.uDu."],
              "full": ["DxuDxuDu", "xuDxuDuD"], "soft": ["d...d.u.", "..d...u."]}
GAINS = {"primeira": -4, "segunda": -5.5, "terceira": -8, "caixa": -9, "repinique": -7, "tamborim": -9,
         "agogo": -16, "ganza": -15, "apito": -17, "cavaco": -12, "violao": -12, "baixo": -6, "flute": -5}
PANS = {"primeira": -0.12, "segunda": 0.14, "terceira": 0.0, "repinique": 0.35, "agogo": 0.55, "apito": 0.08,
        "cavaco": 0.3, "violao": -0.35, "baixo": 0.0, "flute": -0.08}
CAIXA_PANS = [-0.5, 0.48, -0.15]
TAMBORIM_PANS = [-0.45, 0.62]
GANZA_PANS = [-0.25, 0.4]
SWING_MELODY = 0.12


class Score:
    """Collects strokes and notes: (sample, voice, dB, pan, bus, exact)."""

    def __init__(self, rng):
        self.rng, self.events = rng, []

    def add(self, sample, voice, level, pan, bus="room", exact=False):
        self.events.append((int(sample), voice, level, pan, bus, exact))


def at(phrase_start, sixteenth, swing=SWING):
    """The sample a sixteenth of a phrase sounds on, the off ones pushed late."""
    return phrase_start + sixteenth * SIXTEENTH + (round(swing * SIXTEENTH) if sixteenth % 2 else 0)


def notes(text):
    for item in filter(None, (s.strip() for s in text.split(","))):
        pos, name, length = item.split()
        yield int(pos), midi(name), int(length)


def compose(rng):
    sc = Score(rng)
    previous = {"cavaco": None, "violao": None}
    for p in range(1, PHRASES + 1):
        plan = PLAN[p]
        start = PHRASE_FRAMES * (p - 1) * SPF
        bars = [chord(c) for c in CHORDS[p].split()]
        last = p == PHRASES

        def level(b):
            return plan["lift"] + CRESCENDO[min(b, 10) - 1]

        def silent(pos):
            """The breath: nothing but the ganzá on the last sixteenth before the PA."""
            return pos == 39

        def muted(pos, layer):
            """What the fill takes out of the groove."""
            fill = plan["fill"]
            beat = pos // 4 + 1
            if fill in ("surdo-stop", "call-response") and beat == 10:
                return True
            if fill == "dialog" and beat >= 8:
                return layer not in ("comp", "bass")
            if fill == "break" and (beat == 10 or (beat == 9 and pos % 4)):
                return True
            return False

        # --- Harmony: cavaquinho, guitar, bass -------------------------------------------
        cav = {}
        for bar, (root, pcs) in enumerate(bars):
            cav[bar] = voice(pcs, 62, 79, previous["cavaco"])
            previous["cavaco"] = cav[bar]
        for bar in range(8):
            first = bar * 8
            if 40 <= first < 48 and not last:
                continue  # the PA's bar is the PA's own
            if last and first >= 40:
                continue
            style = plan["comp"]
            if style == "picked":
                # Bar by bar, the chord's notes one at a time, an arpeggio rising and falling.
                order = [0, 2, 1, 3, 2, 1, 3, 2]
                for k, idx in enumerate(order):
                    pos = first + k
                    if pos in (40, 41) or silent(pos):
                        continue
                    sc.add(at(start, pos), ("cavaco", cav[bar][idx], 3 * S16), GAINS["cavaco"] - 5 + (0 if k % 2 else 1),
                           PANS["cavaco"], exact=(pos % 4 == 0 and pos < 40))
                continue
            cells = COMP_CELLS[style]
            cell = cells[bar % 2]
            if p in (5, 9) and bar == 3:
                cell = "D.uDxuDu"  # a push into the fifth bar
            strokes = [k for k, ch in enumerate(cell) if ch != "."]
            for i, k in enumerate(strokes):
                pos = first + k
                if silent(pos) or muted(pos, "comp") or (first >= 48 and bar == 6 and k == 0):
                    continue
                ch = cell[k]
                nxt = strokes[i + 1] - k if i + 1 < len(strokes) else 8 - k
                dur = 0.06 if ch == "x" else nxt * S16 + 0.02
                lvl = GAINS["cavaco"] + {"D": 0, "d": -3, "U": -1, "u": -4, "x": -6}[ch] + 0.4 * level(pos // 4 + 1) + 3
                strum(sc, at(start, pos), cav[bar], ch, dur, lvl, exact=(pos % 4 == 0 and pos < 40))
            if plan["guitar"] and bar * 2 + 1 >= plan["guitar"]:
                gv = voice(bars[bar][1][1:], 52, 66, previous["violao"])
                previous["violao"] = gv
                for k, what in ((0, "bass"), (2, "chord"), (4, "fifth"), (5, "chord"), (7, "chord")):
                    pos = first + k
                    if silent(pos) or muted(pos, "comp") or pos in (48,):
                        continue
                    root = 40 + (bars[bar][0] - 40) % 12
                    if what == "chord":
                        for j, m in enumerate(gv):
                            sc.add(at(start, pos) + j * int(0.006 * SR), ("violao", m, 1.6 * S16),
                                   GAINS["violao"] - 2, PANS["violao"])
                    else:
                        m = root if what == "bass" else root + 7
                        sc.add(at(start, pos), ("violao", m, 3 * S16), GAINS["violao"], PANS["violao"],
                               exact=(pos % 4 == 0 and pos < 40))

        # Bass: locked to the surdos, the root on one, the fifth on two, a walk into the next bar.
        if plan["bass"]:
            for bar in range(8):
                first = bar * 8
                if first // 4 + 1 < plan["bass"] or first == 40 or (last and first >= 40):
                    continue
                root = 40 + (bars[bar][0] - 40) % 12
                nxt = 40 + (bars[(bar + 1) % 8][0] - 40) % 12
                figure = [(0, root, 2), (3, root, 1), (4, root + 7, 3)]
                if plan["comp"] in ("groove", "full") and bar != 4:
                    figure.append((7, nxt - 1 if nxt - 1 != root + 7 else nxt + 1, 1))
                for k, m, length in figure:
                    pos = first + k
                    if silent(pos) or muted(pos, "bass") or (first == 48 and k == 0 and False):
                        continue
                    sc.add(at(start, pos), ("baixo", m, length * S16 - 0.01), GAINS["baixo"] + (0 if k in (0, 4) else -4),
                           PANS["baixo"], exact=(pos % 4 == 0 and pos < 40))

        # Flute
        for pos, m, length in notes(MELODY[p]):
            exact = pos in (40, 48)
            sc.add(at(start, pos, SWING_MELODY), ("flute", m, length * S16 - 0.03), GAINS["flute"] + (1.5 if pos == 40 else 0),
                   PANS["flute"], "melody", exact=exact)

        # --- Bateria ---------------------------------------------------------------------
        bateria(sc, p, plan, start, level, silent, muted)

        # --- The PA, the echo ------------------------------------------------------------
        pa = start + 40 * SIXTEENTH
        pa_hit(sc, p, plan, pa, bars[5], cav[5])
        echo = start + 48 * SIXTEENTH
        if last:
            sc.add(echo, "segunda", -14, PANS["segunda"], "last", exact=True)
        else:
            sc.add(echo, "segunda", -8, PANS["segunda"], exact=True)
            strum(sc, echo, cav[6], "x", 0.07, GAINS["cavaco"] - 1, exact=True)
            sc.add(echo, ("baixo", 40 + (bars[6][0] - 40) % 12, 2 * S16), GAINS["baixo"], PANS["baixo"], exact=True)

        # --- After the PA: breath, and a pickup -----------------------------------------
        if not last:
            breath = -26 if plan["ganza"] is None else -22
            for pos in range(50, 64):
                if plan["ganza"] or pos >= 56:
                    sc.add(at(start, pos), "ganza-accent" if pos % 2 else "ganza", breath + (pos - 50) * 0.4, GANZA_PANS[0])
            if plan["lift"] >= -3.5:
                for pos in (60, 62, 63):
                    sc.add(at(start, pos), "tamborim", -24 + plan["lift"], TAMBORIM_PANS[0])
            if plan.get("rebuild"):
                # A caixa roll from beat 13 to 16, from nothing to forte, into the rebuild.
                for j in range(24):
                    sc.add(start + 48 * SIXTEENTH + j * SIXTEENTH // 2, "caixa-ghost" if j < 10 else "caixa-normal",
                           -34 + j * 1.0, CAIXA_PANS[j % 2])
            if plan.get("apito"):
                for pos, length in ((52, 0.12), (54, 0.12), (56, 0.6)):
                    sc.add(at(start, pos), ("apito", length), GAINS["apito"], PANS["apito"])
    return sc.events


def strum(sc, sample, notes_, stroke, dur, level, exact=False):
    """A strum across the cavaquinho's four strings: down from the bottom,
    up from the top and lighter, a chop choked at once."""
    order = list(notes_) if stroke.lower() in ("d", "x") else list(reversed(notes_))[:3]
    spread = 0.007 if stroke == "D" else 0.005 if stroke in ("d", "x") else 0.004
    for j, m in enumerate(order):
        sc.add(sample + int(j * spread * SR), ("cavaco", m, max(0.05, dur - j * spread)), level - 1.5 * j * (stroke.islower()),
               PANS["cavaco"], exact=(exact and j == 0))


def bateria(sc, p, plan, start, level, silent, muted):
    rng = sc.rng
    for b in range(1, 11):
        for k in range(4):
            pos = (b - 1) * 4 + k
            cpos = pos % 8  # the sixteenth in the two-beat bar
            here = at(start, pos)
            down = k == 0
            c = level(b)
            if silent(pos):
                if plan["ganza"]:
                    sc.add(here, "ganza", c + GAINS["ganza"], GANZA_PANS[0])
                continue

            if plan["ganza"] and b >= plan["ganza"][0] and not muted(pos, "ganza"):
                for player in range(plan["ganza"][1]):
                    sc.add(here, "ganza-accent" if k in (0, 1, 3) else "ganza", c + GAINS["ganza"], GANZA_PANS[player],
                           exact=(down and player == 0))

            if plan["tamborim"] and b >= plan["tamborim"][0] and not muted(pos, "tamborim"):
                first, cell, players = plan["tamborim"]
                if p >= 9 and b >= 9:
                    cell = "xxxxxxxx"  # carreteiro: every sixteenth
                if plan["fill"] == "tamborim-run" and b == 10:
                    cell = "xxxxxxxx"
                if TAMBORIM_CELLS.get(cell, cell)[cpos] == "x":
                    for player in range(players):
                        sc.add(here, "tamborim" if k in (0, 3) else "tamborim-muted",
                               c + GAINS["tamborim"] - (0 if down else 3) - 2 * player, TAMBORIM_PANS[player],
                               exact=(down and player == 0))

            if plan["caixa"] and b >= plan["caixa"][0] and not muted(pos, "caixa"):
                first, cell, players = plan["caixa"]
                if players == "steps":
                    players = 1 if b < 5 else 2 if b < 7 else 3
                if plan["fill"] in ("roll", "final") and b >= (9 if plan["fill"] == "roll" else 10):
                    if k < 3 or plan["fill"] == "roll" and b == 9:
                        for half in range(2):
                            j = (pos - (32 if plan["fill"] == "roll" else 36)) * 2 + half
                            for player in range(players):
                                sc.add(here + half * SIXTEENTH // 2, "caixa-normal" if j > 6 else "caixa-ghost",
                                       c + GAINS["caixa"] - 10 + 1.2 * j, CAIXA_PANS[player])
                    continue
                stroke = CAIXA_CELLS[cell][cpos]
                if stroke != ".":
                    if rng.random() < 0.15 and stroke == "g":
                        continue  # a ghost left out
                    voice = {"A": "caixa-accent", "n": "caixa-normal", "g": "caixa-ghost"}[stroke]
                    for player in range(players):
                        sc.add(here, voice, c + GAINS["caixa"], CAIXA_PANS[player], exact=(down and player == 0))

            if plan["surdo"] and b >= plan["surdo"][0] and down and not muted(pos, "surdo"):
                style = plan["surdo"][1]
                if style == "pulse":
                    sc.add(here, "segunda" if b % 2 == 0 else "primeira-muted", c + GAINS["segunda"] - 4,
                           PANS["segunda"], exact=True)
                elif b % 2:
                    sc.add(here, "segunda", c + GAINS["segunda"], PANS["segunda"], exact=True)
                    sc.add(here, "primeira-muted", c + GAINS["primeira"] - 12, PANS["primeira"])
                else:
                    sc.add(here, "primeira", c + GAINS["primeira"], PANS["primeira"], exact=True)
            if plan["terceira"] and b >= plan["terceira"] and not muted(pos, "surdo"):
                if cpos == 0:
                    sc.terceira = TERCEIRA_CELLS[rng.integers(len(TERCEIRA_CELLS))]  # improvised, bar by bar
                ch = sc.terceira[cpos]
                if ch != ".":
                    sc.add(here, "terceira" if ch == "o" else "terceira-muted", c + GAINS["terceira"], PANS["terceira"])

            if plan["agogo"] and b >= plan["agogo"][0] and not muted(pos, "agogo"):
                ch = AGOGO_CELLS[plan["agogo"][1]][cpos]
                if ch != ".":
                    sc.add(here, "agogo-high" if ch == "H" else "agogo-low", c + GAINS["agogo"], PANS["agogo"],
                           exact=down)

        # Fills that add rather than take away.
        fill = plan["fill"]
        calls = {"call-short": {10: "rrr."}, "call-response": {9: "r.rr"}, "call-long": {8: "r.r.", 9: "r.rr", 10: "rrr."},
                 "dialog": {8: "r.rr", 10: "rr.."}, "final": {8: "r.r.", 9: "rrrr", 10: "rrr."}}.get(fill, {})
        if b in calls:
            for k, ch in enumerate(calls[b]):
                if ch == "r":
                    sc.add(at(start, (b - 1) * 4 + k), "repinique-rim" if k in (0, 2) or b == 10 else "repinique",
                           level(b) + GAINS["repinique"] + (1.2 * k if b == 10 else 0), PANS["repinique"], exact=(k == 0))
        beat = start + (b - 1) * 4 * SIXTEENTH
        if fill == "surdo-stop" and b == 10:
            for k in (0, 2):
                sc.add(at(start, 36 + k), "primeira", level(b) + GAINS["primeira"] + 2, PANS["primeira"], exact=(k == 0))
        if fill == "call-response" and b == 10:
            for k in (0, 1):
                for v, g in (("primeira", -2), ("segunda", -3), ("caixa-accent", -8)):
                    sc.add(at(start, 36 + k), v, level(b) + g, PANS.get(v, -0.3), exact=(k == 0))
        if fill == "dialog" and b == 9:
            sc.add(beat, "primeira", level(b) + GAINS["primeira"] + 2, PANS["primeira"], exact=True)
            sc.add(at(start, 34), "segunda", level(b) + GAINS["segunda"] + 2, PANS["segunda"])
        if fill == "break" and b == 9:
            for v, g in (("primeira", -2), ("segunda", -3), ("caixa-accent", -8), ("repinique-rim", -7)):
                sc.add(beat, v, level(b) + g, PANS.get(v, -0.3), exact=True)
        if fill == "break" and b == 10:
            sc.add(beat, "ganza-accent", level(b) + GAINS["ganza"] - 4, GANZA_PANS[0], exact=True)  # the pulse, barely
        if b == 10 and fill == "none":
            pass


def pa_hit(sc, p, plan, pa, bar, cav):
    """The PA: a surdo-and-caixa stab, a tutti, or the final chord."""
    kind = plan["pa"]
    last = kind == "final"
    lift = 2.0 if last else 0.0
    bus = "last" if last else "pa"
    root = 40 + (bar[0] - 40) % 12
    hits = [("primeira", -1), ("segunda", -3), ("caixa-accent", -5)]
    if kind in ("tutti", "final"):
        hits += [("terceira", -4), ("repinique-rim", -4), ("tamborim", -8), ("agogo-low", -11), ("ganza-accent", -14)]
    for v, g in hits:
        pan = PANS.get(v.split("-")[0], 0.0)
        sc.add(pa, v, g + lift, pan, bus, exact=True)
        if v == "caixa-accent":
            for extra in CAIXA_PANS[1 : 3 if kind != "stab" else 2]:
                sc.add(pa, v, g + lift, extra, bus, exact=True)
            sc.add(pa - int(0.018 * SR), "caixa-ghost", g - 12 + lift, CAIXA_PANS[0], bus, exact=True)  # flam
    ring = (16 * S16 if not last else 3.2)
    strum(sc, pa, cav, "D", ring, GAINS["cavaco"] + 3 + lift, exact=True)
    sc.add(pa, ("baixo", root, ring if last else 6 * S16), GAINS["baixo"] + 1 + lift, PANS["baixo"], bus, exact=True)
    if plan["guitar"] or last:
        gv = voice(bar[1], 50, 66, None)
        for j, m in enumerate([root + 12] + gv):
            sc.add(pa + j * int(0.008 * SR), ("violao", m, ring), GAINS["violao"] + lift, PANS["violao"], bus,
                   exact=(j == 0))
    if last:
        # The ring-out: the cavaquinho's tremolo on the last chord, dying away over two beats.
        for j in range(1, 14):
            sc.add(pa + j * SIXTEENTH // 2, ("cavaco", cav[j % 4], 0.25), GAINS["cavaco"] - 2 - 1.6 * j, PANS["cavaco"], bus)
        sc.add(pa, "primeira", 0 + lift, PANS["primeira"], bus, exact=True)


def humanise(rng, events):
    """Players drift a few ms and play a touch louder or softer; the
    downbeats, the PA and the echo stay on the frame."""
    out = []
    for sample, voice, level, pan, bus, exact in events:
        if not exact:
            sample += round(np.clip(rng.normal(0, JITTER_SECONDS / 2.5), -JITTER_SECONDS, JITTER_SECONDS) * SR)
            level += rng.uniform(-JITTER_DB, JITTER_DB)
        out.append((max(0, sample), voice, level, pan, bus))
    return out


def place(bus, at_, x, pan=0.0):
    end = min(len(bus), at_ + len(x))
    left, right = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    bus[at_:end, 0] += x[: end - at_] * left * np.sqrt(2)
    bus[at_:end, 1] += x[: end - at_] * right * np.sqrt(2)


def mix(rng):
    kit = Kit(rng)
    strings_ = Strings(rng)
    events = humanise(rng, compose(rng))
    rooms = {"short": room(rng, 0.9), "long": room(rng, 2.2)}
    dry = np.zeros((LENGTH, 2))
    sends = {"room": np.zeros(LENGTH), "melody": np.zeros(LENGTH), "pa": np.zeros(LENGTH), "last": np.zeros(LENGTH)}
    send_db = {"room": ROOM_SEND, "melody": ROOM_SEND + 3, "pa": PA_SEND, "last": PA_SEND + 2}
    for sample, voice, level, pan, bus in events:
        if isinstance(voice, tuple) and voice[0] == "apito":
            x = apito(rng, voice[1])
        elif isinstance(voice, tuple):
            x = strings_.get(*voice)
        else:
            x = kit[voice]
        x = x * db(level)
        place(dry, sample, x, pan)
        end = min(LENGTH, sample + len(x))
        sends[bus][sample:end] += x[: end - sample] * db(send_db[bus])

    wet = np.zeros((LENGTH, 2))
    for bus, ir in (("room", "short"), ("melody", "short"), ("pa", "short"), ("last", "long")):
        for c in range(2):
            wet[:, c] += oaconvolve(sends[bus], rooms[ir][:, c])[:LENGTH]

    out = dry + wet
    out = sosfilt(butter(2, 25, btype="highpass", fs=SR, output="sos"), out, axis=0)  # no DC, no subsonic drift
    # A short safety fade at the very end only: by then the room has all but died.
    fade = int(FADE_SECONDS * SR)
    out[-fade:] *= np.cos(np.linspace(0, np.pi / 2, fade))[:, None] ** 2
    return out / np.max(np.abs(out)) * db(-3)


def hit_list(raw):
    """hits.json: the downbeats of beats 1 to 10, the PA (11) and the echo,
    each with its level as sounded: the RMS of its first 100 ms against the
    PAs of phrases 1 to 9."""
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
    """An onset envelope (2 ms RMS, centred, every sample); a PA's attack
    starts where the envelope first passes half the peak near its frame,
    well over the call that leads into it."""
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
        at = (lo + np.argmax(seg > 0.5 * seg.max())) / SR
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
