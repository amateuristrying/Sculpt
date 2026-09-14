"""Reproducible development runtime installer. Requires uv and Git on PATH."""
import os
from pathlib import Path
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from sculpt_backend.config import UPSTREAM_REVISION, runtime_root


def main():
    root = runtime_root()
    backend = Path(__file__).resolve().parent
    uv = shutil.which("uv")
    if not uv:
        raise SystemExit("Install uv from https://docs.astral.sh/uv/getting-started/installation/ then run setup again.")
    root.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(root).free < 4 * 1024**3:
        raise SystemExit("Sculpt needs at least 4 GB free for this runtime and model installation.")
    python = root / "venv" / "bin" / "python"
    if not python.exists():
        subprocess.run([uv, "venv", "--python", "3.11", str(root / "venv")], check=True)
    subprocess.run([uv, "pip", "install", "--python", str(python), "--require-hashes", "-r", str(backend / "requirements.lock")], check=True)
    source = root / "TripoSR"
    if not source.exists():
        subprocess.run(["git", "clone", "https://github.com/VAST-AI-Research/TripoSR.git", str(source)], check=True)
    subprocess.run(["git", "-C", str(source), "checkout", "--detach", UPSTREAM_REVISION], check=True)
    subprocess.run([str(python), str(backend / "download_models.py")], check=True, env={**os.environ, "SCULPT_RUNTIME_DIR": str(root)})


if __name__ == "__main__":
    main()
