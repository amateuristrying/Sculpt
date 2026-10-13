"""Explicit one-time download; generation itself is offline."""
from pathlib import Path
import time

from requests.exceptions import ChunkedEncodingError, ConnectionError, HTTPError, Timeout

from sculpt_backend.config import DINO_REVISION, MODEL_REVISION, SPEC, configure_environment, runtime_root
from sculpt_backend.health import checksum, write_verified_manifest
from sculpt_backend.protocol import progress, emit


def download_with_retry(download, *args, progress_value, **kwargs):
    """Resume pinned Hub downloads after transient transport failures only.

    The Hub keeps its partial file; reusing the same revision and destination
    lets it resume. Authentication, missing files, and checksum errors still
    fail immediately. The native supervisor can cancel during the short waits.
    """
    for attempt in range(3):
        try:
            return download(*args, **kwargs)
        except (ChunkedEncodingError, ConnectionError, Timeout, HTTPError) as error:
            if isinstance(error, HTTPError):
                status = error.response.status_code if error.response is not None else None
                if status != 429 and (status is None or not 500 <= status < 600):
                    raise
            if attempt == 2:
                raise
            progress('models', progress_value, f'Download interrupted. Resuming cached download · retry {attempt + 1}/2')
            time.sleep(2 ** (attempt + 1))


def main():
    root = runtime_root()
    configure_environment(root, offline=False)
    from huggingface_hub import hf_hub_download
    import rembg
    import torch

    model_dir = root / "models" / "triposr"
    for index, name in enumerate(("config.yaml", "model.ckpt")):
        progress('models', 50 + index * 8, 'Downloading reconstruction weights (resumes cached downloads)')
        local = model_dir / name
        damaged = local.exists() and checksum(local) != SPEC['artifacts'][f'models/triposr/{name}']
        download_with_retry(hf_hub_download, "stabilityai/TripoSR", name, revision=MODEL_REVISION,
                            local_dir=model_dir, force_download=damaged, progress_value=50 + index * 8)
    progress('models', 72, 'Preparing image encoder configuration')
    # Upstream requests the default revision during construction. Prime that
    # cache alias with the exact known config revision, then lock generation offline.
    path = Path(download_with_retry(hf_hub_download, "facebook/dino-vitb16", "config.json",
                                    revision=DINO_REVISION, progress_value=72))
    refs = path.parents[2] / "refs"
    refs.mkdir(exist_ok=True)
    (refs / "main").write_text(DINO_REVISION)
    progress('models', 80, 'Preparing the background removal model')
    background = root / 'models/background/u2net.onnx'
    if background.exists() and checksum(background) != SPEC['artifacts']['models/background/u2net.onnx']:
        background.unlink()
    rembg.new_session("u2net", providers=["CPUExecutionProvider"])
    progress('verifying', 92, 'Verifying model checksums and local compute')
    write_verified_manifest(root, [str(path.relative_to(root)), str((refs / 'main').relative_to(root))])
    emit('result', installed=True, mps=torch.backends.mps.is_available())


if __name__ == "__main__":
    main()
