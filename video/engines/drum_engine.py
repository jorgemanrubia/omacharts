#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy", "scipy", "soundfile"]
# ///
"""Synthesise the Trading video's drum, by the score in video/scripts/trading.md.

    video/engines/drum_engine.py OUT_DIR

Writes OUT_DIR/drum.wav (48 kHz, 24-bit, stereo, mastered to -14 LUFS) and
OUT_DIR/hits.json, the frame, sounded time and level of every hit, which the
edit reads. Nothing is sampled: every sound is sines and filtered noise.
Deterministic from seed 7.

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
SEED = 7

D1, D2, F2, G2, A2, D3 = 36.708, 73.416, 87.307, 97.999, 110.0, 146.832

# Hits 1..10, then PA (11), then the echo on beat 13.
LEVELS = [-21, -22, -20, -21, -20, -19, -16, -13, -10, -7]
PITCHES = [D2, D2, F2, D2, G2, F2, D2, F2, G2, A2]
ECHO_BEAT, ECHO_LEVEL, ECHO_PITCH = 13, -20, D3

QUIET_SEND, PA_SEND = -16, -6
SWELL_SECONDS, SWELL_LEVEL = 0.35, -12
JITTER_SECONDS, JITTER_DB = 0.004, 0.5
FADE_SECONDS = 1.5
TARGET_I, TARGET_TP = -14.0, -1.0
KNEE = -7.0  # dBFS, where the master's soft knee starts


def db(x):
    return 10 ** (x / 20)


def seconds(n):
    return np.arange(int(n * SR)) / SR


def band(lo, hi, order=2):
    return butter(order, [lo, hi], btype="bandpass", fs=SR, output="sos")


def ramp(x, ms):
    """A raised-cosine onset, so noise never starts on a step."""
    n = max(1, int(ms * SR / 1000))
    x[:n] *= 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / n)
    return x


def tail_off(x, ms=20):
    n = int(ms * SR / 1000)
    x[-n:] *= np.linspace(1, 0, n)
    return x


def peak_to(x, level=1.0):
    return x * (level / np.max(np.abs(x)))


# --- Voices -------------------------------------------------------------------------------


def tom(rng, f):
    """A sine bent down into its pitch from 2.2x (18 ms), a 1.5x partial and a stick."""
    t = seconds(2.0)
    tau = 0.018
    phase = 2 * np.pi * (f * t + f * 1.2 * tau * (1 - np.exp(-t / tau)))
    body = np.sin(phase) * np.exp(-t / 0.22)
    partial = db(-12) * np.sin(1.5 * phase) * np.exp(-t / 0.09)
    stick = np.zeros_like(t)
    n = int(0.012 * SR)
    burst = sosfilt(band(2000, 6000), rng.standard_normal(n + 512))[512:]
    stick[:n] = peak_to(burst * np.exp(-t[:n] / 0.003), db(-14))
    x = ramp(body + partial + stick, 0.3)
    drive = 1.4  # gentle: about 1 dB of rounding on the peak
    return tail_off(np.tanh(drive * x) / np.tanh(drive))


def sub(f):
    """A plain sine an octave under a tom, for weight."""
    t = seconds(1.6)
    return tail_off(ramp(np.sin(2 * np.pi * f * t) * np.exp(-t / 0.30), 3))


def kick(rng):
    t = seconds(2.4)
    tau = 0.030
    phase = 2 * np.pi * (48 * t + 112 * tau * (1 - np.exp(-t / tau)))
    body = np.sin(phase) * np.exp(-t / 0.35)
    n = int(0.003 * SR)
    click = np.zeros_like(t)
    burst = sosfilt(butter(2, 1500, btype="highpass", fs=SR, output="sos"), rng.standard_normal(n + 512))[512:]
    click[:n] = peak_to(burst * np.hanning(n), db(-12))
    return tail_off(ramp(body + click, 0.3))


def snare(rng):
    t = seconds(1.2)
    noise = peak_to(sosfilt(band(180, 8000), rng.standard_normal(len(t) + 512))[512:] * np.exp(-t / 0.16))
    tones = (np.sin(2 * np.pi * 185 * t) + 0.7 * np.sin(2 * np.pi * 330 * t)) * np.exp(-t / 0.08)
    return tail_off(peak_to(ramp(0.8 * noise + 0.45 * peak_to(tones), 0.5)))


def brush(rng):
    """A closed brush tick: a short breath of high noise."""
    t = seconds(0.12)
    noise = sosfilt(band(2500, 11000), rng.standard_normal(len(t) + 512))[512:]
    env = np.exp(-t / 0.018)
    return tail_off(peak_to(ramp(noise * env, 1.5)), 10)


def room(rng, rt60):
    """Decaying noise, different left and right, 12 ms pre-delay, low-passed at 6 kHz."""
    t = seconds(rt60 * 1.3)
    env = np.exp(-6.908 * t / rt60)
    lowpass = butter(2, 6000, btype="lowpass", fs=SR, output="sos")
    # Keep the low end out of the room so the sub and the D1 stay tight.
    highpass = butter(2, 90, btype="highpass", fs=SR, output="sos")
    pre = np.zeros(int(0.012 * SR))
    channels = []
    for _ in range(2):
        ir = ramp(sosfilt(highpass, sosfilt(lowpass, rng.standard_normal(len(t)))) * env, 2)
        ir = np.concatenate([pre, tail_off(ir, 50)])
        channels.append(ir / np.sqrt(np.sum(ir**2)))
    return np.stack(channels, axis=1)


# --- Score --------------------------------------------------------------------------------


def score(rng):
    """Every event: start sample, voice, gain, pan, which room it sends to."""
    events, hits = [], []
    for p in range(1, PHRASES + 1):
        start = PHRASE_FRAMES * (p - 1)
        last = p == PHRASES
        for n in range(1, 11):
            frame = start + FRAMES_PER_BEAT * (n - 1)
            dt, dl = rng.uniform(-JITTER_SECONDS, JITTER_SECONDS), rng.uniform(-JITTER_DB, JITTER_DB)
            if p == 1 and n == 1:
                dt = 0.0  # the first hit is the first frame
            at = frame * SPF + round(dt * SR)
            level = LEVELS[n - 1] + dl
            f = PITCHES[n - 1]
            events.append(("tom", at, f, level, 0.0, "quiet"))
            if p >= 4:
                events.append(("sub", at, f / 2, level - 9, 0.0, "quiet"))
            if p >= 8 and n >= 7:
                events.append(("tom", at, f * 1.5, level - 10, 0.25, "quiet"))
            hits.append({"phrase": p, "hit": n, "frame": frame, "time": round(at / SR, 6), "level_db": round(level, 2)})
            # The brush on the off-beat eighth: -34 dB, rising to -28 over beats 7 to 10.
            brush_level = -34 if n <= 6 else -34 + 1.5 * (n - 6)
            bt = rng.uniform(-JITTER_SECONDS, JITTER_SECONDS)
            bl = rng.uniform(-JITTER_DB, JITTER_DB)
            events.append(("brush", frame * SPF + 13 * SPF + round(bt * SR), None, brush_level + bl,
                           0.3 if n % 2 else -0.3, "quiet"))
        pa = start + FRAMES_PER_BEAT * 10
        gain = 2.0 if last else 0.0
        events.append(("pa", pa * SPF, p, gain, 0.0, "last" if last else "pa"))
        hits.append({"phrase": p, "hit": 11, "frame": pa, "time": round(pa * SPF / SR, 6), "level_db": gain})
        echo = start + FRAMES_PER_BEAT * (ECHO_BEAT - 1)
        events.append(("tom", echo * SPF, ECHO_PITCH, ECHO_LEVEL, -0.15, "quiet"))
        if p >= 4:
            events.append(("sub", echo * SPF, ECHO_PITCH / 2, ECHO_LEVEL - 9, 0.0, "quiet"))
        hits.append({"phrase": p, "hit": "echo", "frame": echo, "time": round(echo * SPF / SR, 6), "level_db": ECHO_LEVEL})
    return events, hits


def pa_voice(rng, phrase):
    """PA: a tom on D2 over one on D1, the kick and the snare, at 0 dB."""
    parts = [tom(rng, D2), db(-3) * tom(rng, D1), kick(rng), db(-4) * snare(rng)]
    if phrase >= 4:
        parts.append(db(-9) * sub(D2 / 2))  # under D1 would be 18 Hz: nothing to hear
    out = np.zeros(max(len(x) for x in parts))
    for x in parts:
        out[: len(x)] += x
    return out


def place(bus, at, x, pan=0.0):
    end = min(len(bus), at + len(x))
    left, right = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    bus[at:end, 0] += x[: end - at] * left * np.sqrt(2)
    bus[at:end, 1] += x[: end - at] * right * np.sqrt(2)


def mix(rng):
    events, hits = score(rng)
    rooms = {"short": room(rng, 0.9), "long": room(rng, 1.8)}
    dry = np.zeros((LENGTH, 2))
    sends = {"quiet": np.zeros(LENGTH), "pa": np.zeros(LENGTH), "last": np.zeros(LENGTH)}
    pas = []
    for kind, at, arg, level, pan, send in events:
        if kind == "pa":
            x = pa_voice(rng, arg) * db(level)
            pas.append((at, x, send))
        elif kind == "tom":
            x = tom(rng, arg) * db(level)
        elif kind == "sub":
            x = sub(arg) * db(level)
        else:
            x = brush(rng) * db(level)
        place(dry, at, x, pan)
        end = min(LENGTH, at + len(x))
        sends[send][at:end] += x[: end - at] * db(QUIET_SEND if send == "quiet" else PA_SEND)

    wet = np.zeros((LENGTH, 2))
    for send, ir in (("quiet", "short"), ("pa", "short"), ("last", "long")):
        for c in range(2):
            wet[:, c] += oaconvolve(sends[send], rooms[ir][:, c])[:LENGTH]

    # The swell: each PA's own room tail, reversed, rising into the hit.
    pre = int(0.012 * SR)
    n = int(SWELL_SECONDS * SR)
    for at, x, send in pas:
        ir = rooms["long" if send == "last" else "short"]
        swell = np.stack([oaconvolve(x, ir[:, c])[pre : pre + n] for c in range(2)], axis=1)[::-1]
        swell *= db(SWELL_LEVEL) * np.abs(x).max() / np.abs(swell).max()  # its peak 12 dB under the PA's
        shape = np.sin(np.linspace(0, np.pi / 2, n)) ** 2
        swell *= shape[:, None]
        wet[at - n : at] += swell

    out = dry + wet
    hp = butter(2, 22, btype="highpass", fs=SR, output="sos")  # no DC, no subsonic drift
    out = sosfilt(hp, out, axis=0)
    out -= out.mean(axis=0)
    fade = int(FADE_SECONDS * SR)
    out[-fade:] *= np.cos(np.linspace(0, np.pi / 2, fade))[:, None] ** 2
    return out / np.max(np.abs(out)) * db(-3), hits


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
        # soft knee that rounds the PA's stacked low peaks (they carry little
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
    """An onset envelope (2 ms RMS, centred, every sample); an attack starts where the
    envelope first passes a share of the peak near the hit: 10% for the first
    hit, 50% for a PA, which is well over its swell."""
    window = int(0.002 * SR)
    power = np.cumsum(np.concatenate([[0.0], (x**2).mean(axis=1)]))
    # Centred: env[i] is the 2 ms around sample i.
    env = np.concatenate([np.zeros(window // 2), np.sqrt(np.maximum(power[window:] - power[:-window], 0) / window)])

    def attack(nominal, share, before=0.1, after=0.1):
        lo, hi = max(0, int((nominal - before) * SR)), int((nominal + after) * SR)
        seg = env[lo:hi]
        return (lo + np.argmax(seg > share * seg.max())) / SR, seg

    first, _ = attack(0.0, 0.1, after=0.05)
    mono = np.abs(x).max(axis=1)
    audible = np.argmax(mono > db(-40) * mono[: int(0.05 * SR)].max())
    print(f"phrase 1 hit 1: attack starts at {first * 1000:.2f} ms; first sample within 40 dB of its peak: {audible}")
    worst = 0.0
    for h in hits:
        if h["hit"] != 11:
            continue
        nominal = h["frame"] * SPF / SR
        at, seg = attack(nominal, 0.5)
        lead = env[int((nominal - 0.005) * SR)]
        offset = (at - nominal) * 1000
        worst = max(worst, abs(offset))
        if h["phrase"] in (1, 5, 10):
            print(f"PA phrase {h['phrase']:2d}: frame {h['frame']} = {nominal:.4f} s, attack at {at:.4f} s ({offset:+.2f} ms); "
                  f"5 ms before it the swell is {20 * np.log10(lead / seg.max()):.1f} dB under the hit")
    print(f"worst PA offset across all ten: {worst:.2f} ms (one frame is 16.7 ms)")
    return first, worst


def spectrum_check(x):
    dc = x.mean(axis=0)
    spec = np.abs(np.fft.rfft(x.mean(axis=1)))
    freqs = np.fft.rfftfreq(len(x), 1 / SR)
    total = np.sum(spec**2)
    share = lambda lo, hi: 10 * np.log10(np.sum(spec[(freqs >= lo) & (freqs < hi)] ** 2) / total + 1e-20)
    print(f"DC L {dc[0]:+.2e} R {dc[1]:+.2e}; energy below 30 Hz {share(0, 30):.1f} dB, "
          f"30-200 Hz {share(30, 200):.1f} dB, 2-8 kHz {share(2000, 8000):.1f} dB, above 16 kHz {share(16000, 24000):.1f} dB; "
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
        cols = np.array_split(y, w)
        for c, seg in enumerate(cols):
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
    raw, hits = mix(rng)
    sf.write(out / "drum-raw.wav", raw.astype(np.float32), SR, subtype="FLOAT")
    x, final = master(out / "drum-raw.wav", out / "drum.wav")
    (out / "hits.json").write_text(json.dumps({"fps": FPS, "frames": FRAMES, "frame_per_beat": FRAMES_PER_BEAT,
                                               "phrase_frames": PHRASE_FRAMES, "hits": hits}, indent=1) + "\n")
    first, worst = onset_check(x, hits)
    spectrum_check(x)
    png(out / "waveform.png", x, hits)
    print(f"{len(x)} samples, {len(x) / SR:.4f} s, {len(hits)} hits")
    problems = []
    if float(final["input_tp"]) > TARGET_TP:
        problems.append("true peak over the ceiling")
    if abs(float(final["input_i"]) - TARGET_I) > 0.5:
        problems.append("loudness off target")
    if first > 0.002:
        problems.append("the first hit is late")
    if worst > 1000 / FPS:
        problems.append("a PA is off its frame")
    if problems:
        sys.exit("; ".join(problems))


if __name__ == "__main__":
    main()
