from pathlib import Path
import os

UPSTREAM_REVISION = "107cefdc244c39106fa830359024f6a2f1c78871"
MODEL_REVISION = "5b521936b01fbe1890f6f9baed0254ab6351c04a"
DINO_REVISION = "f205d5d8e640a89a2b8ef0369670dfc37cc07fc2"


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
    # MPS unsupported operations may run on CPU, but never silently rerun an
    # entire failed model or raise the OS memory watermark.
    os.environ["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
    if offline:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
