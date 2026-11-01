"""Compose instrumental candidates with ACE-Step 1.5.

Run inside ACE-Step's own environment (`bin/video setup` clones it and builds
it), with the checkout and a job file:

    {"caption": "...", "duration": 64, "count": 3, "out": "folder"}

Each candidate gets its own seed, so they differ; the seed is in the file
name, so a candidate can be made again.
"""

import json
import os
import shutil
import sys
import time


def main():
    root, job_path = sys.argv[1], sys.argv[2]
    with open(job_path) as f:
        job = json.load(f)
    sys.path.insert(0, root)
    from acestep.handler import AceStepHandler
    from acestep.inference import GenerationConfig, GenerationParams, generate_music
    from acestep.llm_inference import LLMHandler

    dit = AceStepHandler()
    dit.initialize_service(project_root=root, config_path="acestep-v15-turbo", device="cpu")
    llm = LLMHandler()
    llm.initialize(checkpoint_dir=os.path.join(root, "checkpoints"), lm_model_path="acestep-5Hz-lm-1.7B", backend="pt", device="cpu")
    for i in range(job["count"]):
        seed = 1000 + i * 37
        started = time.monotonic()
        params = GenerationParams(caption=job["caption"], instrumental=True, duration=job["duration"], seed=seed)
        result = generate_music(dit, llm, params, GenerationConfig(batch_size=1, audio_format="flac"), save_dir=os.path.join(job["out"], ".work"))
        if not result.success:
            print("failed:", result.error, flush=True)
            continue
        target = os.path.join(job["out"], f"candidate-{i + 1}-seed{seed}.flac")
        shutil.move(result.audios[0]["path"], target)
        print(f"composed {target} in {time.monotonic() - started:.0f}s", flush=True)
    shutil.rmtree(os.path.join(job["out"], ".work"), ignore_errors=True)


if __name__ == "__main__":
    main()
