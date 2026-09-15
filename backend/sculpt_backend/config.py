from pathlib import Path
import os
import json

SPEC = json.loads((Path(__file__).resolve().parents[1] / "runtime-spec.json").read_text())
UPSTREAM_REVISION = SPEC["sourceRevision"]
MODEL_REVISION = SPEC["modelRevision"]
DINO_REVISION = SPEC["dinoRevision"]


def runtime_root() -> Path:
    default = Path(__file__).resolve().parents[2] / ".sculpt-runtime"
    return Path(os.environ.get("SCULPT_RUNTIME_DIR", default)).resolve()


def configure_environment(root: Path, offline: bool = True) -> None:
    os.environ["HF_HOME"] = str(root / "cache" / "huggingface")
    os.environ["U2NET_HOME"] = str(root / "models" / "background")
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    os.environ["HF_HUB_DISABLE_XET"] = "1"
    os.environ["DO_NOT_TRACK"] = "1"
    os.environ["TOKENIZERS_PARALLELISM"] = "false"
    os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
    # MPS unsupported operations may run on CPU, but never silently rerun an
    # entire failed model or raise the OS memory watermark.
    os.environ["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
    if offline:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
    else:
        os.environ.pop("HF_HUB_OFFLINE", None)
        os.environ.pop("TRANSFORMERS_OFFLINE", None)
