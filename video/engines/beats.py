"""Read a track's beat: its tempo, where its beats and downbeats fall, and
how loud each bar is, so a music-driven video can be timed to it.

Run with any environment that has librosa (the qwen one does):

    python beats.py TRACK.flac OUT.json

The beats come from librosa's tracker. A composed track keeps its tempo, so
a straight grid is fitted through the tracked beats and its drift reported:
the grid is what the video is timed to, the tracked beats are the check. The
downbeat is the beat phase (of four) whose beats carry the most low-end
onset energy — where the kick lands on a 4/4 track."""

import json
import sys

import librosa
import numpy as np


def fit_grid(onset, sr, hop, tempo, duration):
    env = onset / (onset.max() or 1)
    rate = sr / hop
    best = (-1, 60 / tempo, 0.0)
    centres = {60 / tempo, 60 / round(tempo)}
    for centre in centres:
        for period in np.linspace(centre * 0.99, centre * 1.01, 201):
            count = int(duration / period)
            for phase in np.arange(0, period, 0.004):
                idx = np.round((phase + np.arange(count) * period) * rate).astype(int)
                idx = idx[idx < len(env) - 1]
                score = np.maximum(env[idx], env[idx + 1]).sum()
                if score > best[0]:
                    best = (score, period, phase)
    return float(best[1]), float(best[2])


def main():
    track, out = sys.argv[1], sys.argv[2]
    y, sr = librosa.load(track, sr=22050, mono=True)
    duration = len(y) / sr
    hop = 256
    onset = librosa.onset.onset_strength(y=y, sr=sr, hop_length=hop)
    tempo, frames = librosa.beat.beat_track(onset_envelope=onset, sr=sr, hop_length=hop, units="frames")
    beats = librosa.frames_to_time(frames, sr=sr, hop_length=hop)
    tempo = float(np.atleast_1d(tempo)[0])

    # A straight grid laid where it catches the most onset energy: the
    # tracker's own beats wander by tens of milliseconds, and a track
    # composed to a tempo keeps it, so its period and phase are searched for
    # directly, around the tracker's tempo and the round tempo nearest it.
    period, first = fit_grid(onset, sr, hop, tempo, duration)
    n = np.arange(len(beats))
    nearest = np.round((beats - first) / period)
    drift = beats - (first + nearest * period)

    # Low-end onsets, for the downbeat: the kick drum.
    low = librosa.onset.onset_strength(y=y, sr=sr, hop_length=hop, fmax=200, n_mels=32)
    times = librosa.frames_to_time(np.arange(len(low)), sr=sr, hop_length=hop)
    count = int((duration - first) / period) + 1
    strength = []
    for i in range(count):
        t = first + i * period
        window = (times >= t - 0.04) & (times <= t + 0.06)
        strength.append(float(low[window].max()) if window.any() else 0.0)
    strength = np.array(strength)
    phases = [float(strength[p::4].mean()) for p in range(4)]
    phase = int(np.argmax(phases))
    downbeat = first + phase * period

    # Loudness per bar, to find the build, the drop and the break.
    rms = librosa.feature.rms(y=y, hop_length=hop)[0]
    rtimes = librosa.frames_to_time(np.arange(len(rms)), sr=sr, hop_length=hop)
    bars = []
    t, bar = downbeat, 1
    while t < duration:
        window = (rtimes >= t) & (rtimes < t + 4 * period)
        level = 20 * np.log10(max(float(rms[window].mean()) if window.any() else 1e-6, 1e-6))
        kicks = strength[phase + (bar - 1) * 4 : phase + bar * 4]
        bars.append({"bar": bar, "t": round(t, 3), "db": round(level, 1), "kick": round(float(kicks.mean()) if len(kicks) else 0, 2)})
        t += 4 * period
        bar += 1

    result = {
        "track": track,
        "duration": round(duration, 3),
        "tempo_tracker": round(tempo, 2),
        "bpm": round(60 / period, 3),
        "period": round(float(period), 5),
        "first_beat": round(float(first), 4),
        "downbeat": round(float(downbeat), 4),
        "phase_strength": [round(p, 2) for p in phases],
        "drift_ms": {"mean_abs": round(float(np.abs(drift).mean()) * 1000, 1), "max_abs": round(float(np.abs(drift).max()) * 1000, 1)},
        "tracked_beats": [round(float(b), 3) for b in beats],
        "bars": bars,
    }
    with open(out, "w") as f:
        json.dump(result, f, indent=1)
    print(f"{result['bpm']} BPM (tracker {result['tempo_tracker']}), downbeat at {result['downbeat']}s, "
          f"drift {result['drift_ms']['mean_abs']}ms mean / {result['drift_ms']['max_abs']}ms max, {len(bars)} bars")


if __name__ == "__main__":
    main()
