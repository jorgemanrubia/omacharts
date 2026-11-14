#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "scipy", "soundfile"]
# ///
"""Synthesise the Trading video's drum: an original samba groove for a bateria.

    video/engines/drum_engine.py OUT_DIR

Writes OUT_DIR/drum.wav (48 kHz, 24-bit, stereo, mastered to -14 LUFS) and
OUT_DIR/hits.json, the frame, sounded time and level of every hit, which the
edit reads. Nothing is sampled: every drum is a struck membrane (inharmonic
Bessel modes, a pitch drop, a stick or mallet transient), the agogô is
metallic partials, the ganzá and the snares are filtered noise. Deterministic
from seed 7.

The grid is the score's (video/scripts/trading.md): 26 frames a beat at 60
fps, sixteen beats a phrase, ten phrases and a 170-frame tail. Each phrase
builds over beats 1 to 10, layer on layer, and lands on beat 11, the PA, with
the whole bateria on the frame; beat 13 is the echo, one surdo answer; beats
12 to 16 breathe. The accented downbeats of beats 1 to 10, the PA and the echo
sound exactly on their frames; everything else is played, a little early or
late, with samba's late second and fourth sixteenths.

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

# The crescendo over beats 1 to 10, in dB under the PA, before the layers add up.
CRESCENDO = [-8, -7.5, -6.5, -6, -5, -4.5, -3.5, -2.5, -1.5, -0.5]
ECHO_LEVEL = -6
BREATH_LEVEL = -24

ROOM_SEND, PA_SEND = -14, -8
JITTER_SECONDS, JITTER_DB = 0.004, 0.8
FADE_SECONDS = 1.5
TARGET_I, TARGET_TP = -14.0, -1.0
KNEE = -7.0  # dBFS, where the master's soft knee starts

# Free circular membrane: the ratios of its first modes (Bessel zeros).
MODES = np.array([1.0, 1.594, 2.136, 2.296, 2.653, 2.918, 3.156, 3.501, 3.600, 3.652])

# Where each player stands, -1 left to 1 right, and how loud the section is.
PANS = {"primeira": -0.12, "segunda": 0.14, "terceira": 0.0, "repinique": 0.32, "agogo": 0.55, "apito": 0.08}
CAIXA_PANS = [-0.5, 0.48, -0.12]
TAMBORIM_PANS = [-0.42, 0.62]
GANZA_PANS = [-0.28, 0.34]
GAINS = {"primeira": 0, "segunda": -1.5, "terceira": -4, "caixa": -9, "repinique": -5, "tamborim": -9,
         "agogo": -13, "ganza": -15, "apito": -14}


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


# --- Score ----------------------------------------------------------------------------------


def stage(p):
    """Phrases 1-3 as written; 4-7 add the terceira, a second caixa and a
    second tamborim; 8-10 are the whole bateria from beat 3."""
    return 1 if p <= 3 else 2 if p <= 7 else 3


# Two-beat cells, one entry per sixteenth.
TAMBORIM = [1, 0, 1, 1, 1, 0, 1, 0]  # into the beat, then away
TAMBORIM_TELECO = [1, 0, 1, 1, 0, 1, 1, 0]  # the syncopated cell once the caixa runs
CAIXA = "AgnA gnAg".replace(" ", "")
AGOGO = ["H", None, "L", "H", None, "L", None, "L"]
TERCEIRA = [None, None, "open", None, "muted", None, None, "open"]
REPINIQUE_CALL = {7: "..rr", 8: "r.r.", 9: "r.rr", 10: "rrr."}


def score(rng):
    """Every stroke: (sample, voice, dB, pan, bus, exact)."""
    events = []

    def at(beat_start, k):
        return beat_start + k * SIXTEENTH + (round(SWING * SIXTEENTH) if k % 2 else 0)

    def hit(sample, voice, level, pan, bus="room", exact=False):
        events.append((sample, voice, level, pan, bus, exact))

    def breath(b, k, voice):
        """The bateria breathes in on the last sixteenth before the PA; only the ganzá keeps going."""
        return b == 10 and k == 3 and not voice.startswith("ganza")

    for p in range(1, PHRASES + 1):
        s = stage(p)
        start = PHRASE_FRAMES * (p - 1)
        for b in range(1, 11):
            beat = (start + FRAMES_PER_BEAT * (b - 1)) * SPF
            c = CRESCENDO[b - 1]
            odd = b % 2 == 1
            cell = range(4) if odd else range(4, 8)

            # Ganzá: every sixteenth, pushed on the offbeats, accented on the beat too.
            for player, pan in enumerate(GANZA_PANS[: 1 if s == 1 else 2]):
                for k in range(4):
                    accent = k in (0, 1, 3)
                    hit(at(beat, k), "ganza-accent" if accent else "ganza", c + GAINS["ganza"],
                        pan, exact=(k == 0 and player == 0))

            # Tamborim: the cell, carreteiro (every sixteenth) at the top in the full bateria.
            players = 1 if s == 1 else 2
            pattern = TAMBORIM if b <= 2 or (s == 1 and b <= 4) else TAMBORIM_TELECO
            for player in range(players):
                for i, k in zip(cell, range(4)):
                    if s == 3 and b >= 9:
                        play, voice = True, "tamborim" if k in (0, 3) else "tamborim-muted"
                    else:
                        play, voice = pattern[i] or k == 0, "tamborim"
                    if play and not breath(b, k, voice):
                        lift = 0 if k == 0 else -3
                        hit(at(beat, k), voice, c + GAINS["tamborim"] + lift - 2 * player, TAMBORIM_PANS[player],
                            exact=(k == 0 and player == 0))

            # Caixa: from beat 3, continuous sixteenths with ghosts; a roll on beat 10 later on.
            if b >= 3:
                for player, pan in enumerate(CAIXA_PANS[:s]):
                    if b == 10 and s >= 2:
                        for j in range(6):  # 32nds over three sixteenths, swelling, then air
                            hit(beat + j * SIXTEENTH // 2, "caixa-ghost" if j < 2 else "caixa-normal" if j < 4 else "caixa-accent",
                                c + GAINS["caixa"] - 9 + 2 * j, pan, exact=(j == 0 and player == 0))
                        continue
                    for i, k in zip(cell, range(4)):
                        if breath(b, k, "caixa"):
                            continue
                        voice = {"A": "caixa-accent", "n": "caixa-normal", "g": "caixa-ghost"}[CAIXA[i]]
                        if k == 0:
                            voice = "caixa-accent"
                        hit(at(beat, k), voice, c + GAINS["caixa"], pan, exact=(k == 0 and player == 0))

            # Surdos: the segunda answers on the odd beats, the primeira marks the even ones.
            if b >= (3 if s == 3 else 5):
                if odd:
                    hit(beat, "segunda", c + GAINS["segunda"], PANS["segunda"], exact=True)
                    hit(at(beat, 0), "primeira-muted", c + GAINS["primeira"] - 12, PANS["primeira"])
                else:
                    hit(beat, "primeira", c + GAINS["primeira"], PANS["primeira"], exact=True)
                if s >= 2 and b >= 5:
                    for i, k in zip(cell, range(4)):
                        if TERCEIRA[i] and not breath(b, k, "terceira"):
                            voice = "terceira" if TERCEIRA[i] == "open" else "terceira-muted"
                            hit(at(beat, k), voice, c + GAINS["terceira"], PANS["terceira"])

            # Agogô: from beat 7, earlier as the video grows.
            if b >= {1: 7, 2: 5, 3: 3}[s]:
                for i, k in zip(cell, range(4)):
                    if AGOGO[i] and not breath(b, k, "agogo"):
                        hit(at(beat, k), "agogo-high" if AGOGO[i] == "H" else "agogo-low", c + GAINS["agogo"],
                            PANS["agogo"], exact=(k == 0))

            # Repinique: its call leads into the PA.
            call = REPINIQUE_CALL.get(b)
            if call and (b >= 8 or s == 3):
                for k, ch in enumerate(call):
                    if ch == "r":
                        hit(at(beat, k), "repinique-rim" if k in (0, 2) or b == 10 else "repinique",
                            c + GAINS["repinique"] + (1.5 * k if b == 10 else 0), PANS["repinique"],
                            exact=(k == 0))

        # The PA: every player, accented, on the frame.
        last = p == PHRASES
        pa = (start + FRAMES_PER_BEAT * 10) * SPF
        lift = 2.0 if last else 0.0
        bus = "last" if last else "pa"
        for voice, level, pan in (
            ("primeira", -2, PANS["primeira"]), ("segunda", -4, PANS["segunda"]), ("terceira", -6, PANS["terceira"]),
            ("repinique-rim", -3, PANS["repinique"]), ("agogo-low", -10, PANS["agogo"]),
            ("ganza-accent", -14, GANZA_PANS[0]),
        ):
            hit(pa, voice, level + lift, pan, bus, exact=True)
        for pan in CAIXA_PANS[:s]:
            hit(pa - int(0.018 * SR), "caixa-ghost", -18 + lift, pan, bus, exact=True)  # the flam's grace note
            hit(pa, "caixa-accent", -6 + lift, pan, bus, exact=True)
        for pan in TAMBORIM_PANS[: 1 if s == 1 else 2]:
            hit(pa, "tamborim", -7 + lift, pan, bus, exact=True)

        # The echo: one segunda, open, alone.
        echo = (start + FRAMES_PER_BEAT * 12) * SPF
        hit(echo, "segunda", ECHO_LEVEL, PANS["segunda"], exact=True)

        # Beats 13 to 16 breathe: the ganzá alone, quietly, finding the next phrase.
        if not last:
            for b in range(13, 17):
                beat = (start + FRAMES_PER_BEAT * (b - 1)) * SPF
                for k in range(4) if b > 13 else range(2, 4):
                    hit(at(beat, k), "ganza-accent" if k % 2 else "ganza", BREATH_LEVEL + 1.5 * (b - 13),
                        GANZA_PANS[0])

        # Once, going into the last phrase: the apito, short, short, long.
        if p == 9:
            for beat_no, k, length in ((14, 0, 0.12), (14, 2, 0.12), (15, 0, 0.55)):
                beat = (start + FRAMES_PER_BEAT * (beat_no - 1)) * SPF
                hit(at(beat, k), ("apito", length), GAINS["apito"], PANS["apito"])
    return events


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


def place(bus, at, x, pan=0.0):
    end = min(len(bus), at + len(x))
    left, right = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    bus[at:end, 0] += x[: end - at] * left * np.sqrt(2)
    bus[at:end, 1] += x[: end - at] * right * np.sqrt(2)


def mix(rng):
    kit = Kit(rng)
    events = humanise(rng, score(rng))
    rooms = {"short": room(rng, 0.9), "long": room(rng, 1.8)}
    dry = np.zeros((LENGTH, 2))
    sends = {"room": np.zeros(LENGTH), "pa": np.zeros(LENGTH), "last": np.zeros(LENGTH)}
    for sample, voice, level, pan, bus in events:
        x = apito(rng, voice[1]) if isinstance(voice, tuple) else kit[voice]
        x = x * db(level)
        place(dry, sample, x, pan)
        end = min(LENGTH, sample + len(x))
        sends[bus][sample:end] += x[: end - sample] * db(ROOM_SEND if bus == "room" else PA_SEND)

    wet = np.zeros((LENGTH, 2))
    for bus, ir in (("room", "short"), ("pa", "short"), ("last", "long")):
        for c in range(2):
            wet[:, c] += oaconvolve(sends[bus], rooms[ir][:, c])[:LENGTH]

    out = dry + wet
    out = sosfilt(butter(2, 25, btype="highpass", fs=SR, output="sos"), out, axis=0)  # no DC, no subsonic drift
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
