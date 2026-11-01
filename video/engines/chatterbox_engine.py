"""Speak a batch of lines with Chatterbox, and time every word in them.

Run inside the chatterbox environment (`bin/video setup` builds it), with a
job file as the only argument:

    {"voice": {"reference": "narrator.wav", "exaggeration": 0.5, ...},
     "jobs": [{"text": "...", "words": ["...", ...], "out": "a.wav"}, ...]}

Each job writes `out` and, beside it, `out` with `.json` for `.wav`: the start
and end of every entry in `words`, found by forced alignment against the
audio that was just made. The words are the narration as spoken, already
normalised to what the aligner can read; the caller keeps the mapping back
to its cues.

Alignment runs in the same process because it needs torch too, and loading
torch once for both halves is most of the cost of a short batch.
"""

import json
import sys

import torch
import torchaudio

from align import align


def speak(jobs, voice):
    from chatterbox.tts import ChatterboxTTS

    model = ChatterboxTTS.from_pretrained(device="cpu")
    settings = {
        "exaggeration": voice.get("exaggeration", 0.5),
        "cfg_weight": voice.get("cfg_weight", 0.5),
        "temperature": voice.get("temperature", 0.8),
    }
    reference = voice.get("reference")
    if reference:
        model.prepare_conditionals(reference, exaggeration=settings["exaggeration"])
    for job in jobs:
        torch.manual_seed(voice.get("seed", 1))
        wav = model.generate(job["text"], **settings)
        torchaudio.save(job["out"], wav, model.sr)
        print("spoke", job["out"], flush=True)


def main():
    with open(sys.argv[1]) as f:
        batch = json.load(f)
    speak(batch["jobs"], batch.get("voice", {}))
    align(batch["jobs"])


if __name__ == "__main__":
    main()
