"""Time every word of a spoken line, by forced alignment.

Shared by the engines: each one speaks its batch, then hands the same jobs
here. MMS_FA reads lowercase letters and apostrophes only, so the words come
in already normalised; the caller maps them back to its cues.
"""

import json

import soundfile
import torch
import torchaudio


def align(jobs):
    bundle = torchaudio.pipelines.MMS_FA
    model = bundle.get_model(with_star=False)
    tokenizer = bundle.get_tokenizer()
    aligner = bundle.get_aligner()
    for job in jobs:
        # soundfile, not torchaudio.load: newer torchaudio hands loading to
        # torchcodec, which these environments do not carry.
        data, rate = soundfile.read(job["out"], dtype="float32", always_2d=True)
        wav = torch.from_numpy(data.T).mean(0, keepdim=True)
        wav = torchaudio.functional.resample(wav, rate, bundle.sample_rate)
        with torch.inference_mode():
            emission, _ = model(wav)
            spans = aligner(emission[0], tokenizer(job["words"]))
        ratio = wav.size(1) / emission.size(1) / bundle.sample_rate
        times = [
            {"word": word, "start": span[0].start * ratio, "end": span[-1].end * ratio}
            for word, span in zip(job["words"], spans)
        ]
        duration = wav.size(1) / bundle.sample_rate
        with open(job["out"][: -len(".wav")] + ".json", "w") as f:
            json.dump({"duration": duration, "words": times}, f)
