"""Explicit one-time download; generation itself is offline."""
import json
from pathlib import Path
import sys

from sculpt_backend.config import DINO_REVISION, MODEL_REVISION, UPSTREAM_REVISION, configure_environment, runtime_root


def main():
    root = runtime_root()
    configure_environment(root, offline=False)
    from huggingface_hub import hf_hub_download
    import rembg
    import torch

    model_dir = root / "models" / "triposr"
    for name in ("config.yaml", "model.ckpt"):
        print(f"Downloading TripoSR {name}…", flush=True)
        hf_hub_download("stabilityai/TripoSR", name, revision=MODEL_REVISION, local_dir=model_dir)
    print("Downloading the image encoder configuration…", flush=True)
    # Upstream requests the default revision during construction. Prime that
    # cache alias with the exact known config revision, then lock generation offline.
    path = Path(hf_hub_download("facebook/dino-vitb16", "config.json", revision=DINO_REVISION))
    refs = path.parents[2] / "refs"
    refs.mkdir(exist_ok=True)
    (refs / "main").write_text(DINO_REVISION)
    print("Downloading local background segmentation weights…", flush=True)
    rembg.new_session("u2net", providers=["CPUExecutionProvider"])
    (root / "ready.json").write_text(json.dumps({
        "engine": "triposr", "modelRevision": MODEL_REVISION,
        "sourceRevision": UPSTREAM_REVISION, "torchVersion": torch.__version__,
        "mpsAvailable": torch.backends.mps.is_available(),
    }, indent=2))
    print(f"Ready. Metal available: {torch.backends.mps.is_available()}", flush=True)


if __name__ == "__main__":
    main()
