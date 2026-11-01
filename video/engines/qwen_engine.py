"""Speak a batch of lines with Qwen3-TTS, cloning a reference voice, and time
every word in them.

Same job file as `chatterbox_engine.py`. Qwen needs the reference's
transcript as well as its audio, so the voice carries both:

    {"voice": {"reference": "narrator.wav", "reference_text": "...", "language": "English"},
     "jobs": [...]}
"""

import json
import sys

import soundfile
import torch

from align import align


def speak(jobs, voice):
    from qwen_tts import Qwen3TTSModel

    model = Qwen3TTSModel.from_pretrained(
        voice.get("model", "Qwen/Qwen3-TTS-12Hz-1.7B-Base"), device_map="cpu", dtype=torch.float32
    )
    prompt = model.create_voice_clone_prompt(
        ref_audio=voice["reference"], ref_text=voice["reference_text"]
    )
    for job in jobs:
        torch.manual_seed(voice.get("seed", 1))
        wavs, rate = model.generate_voice_clone(
            text=job["text"], language=voice.get("language", "English"), voice_clone_prompt=prompt
        )
        soundfile.write(job["out"], wavs[0], rate)
        print("spoke", job["out"], flush=True)


def main():
    with open(sys.argv[1]) as f:
        batch = json.load(f)
    speak(batch["jobs"], batch["voice"])
    align(batch["jobs"])


if __name__ == "__main__":
    main()
