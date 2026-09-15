"""Run the same isolated reconstruction worker used by the desktop app."""
import argparse
import json
from pathlib import Path
import subprocess
from sculpt_backend.config import runtime_root

parser = argparse.ArgumentParser()
parser.add_argument("image", type=Path)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--quality", choices=["draft", "balanced", "high"], default="draft")
parser.add_argument("--device", choices=["auto", "mps", "cpu"], default="auto")
parser.add_argument('--background', choices=['auto', 'keep'], default='auto')
args = parser.parse_args()
output = args.output.resolve()
output.parent.mkdir(parents=True, exist_ok=True)
request = output.parent / "request.json"
request.write_text(json.dumps({"sourcePath": str(args.image.resolve()), "outputPath": str(output), "quality": args.quality, "device": args.device, "background": args.background}))
raise SystemExit(subprocess.call([str(runtime_root() / "venv/bin/python"), str(Path(__file__).parent / "worker.py"), "--request", str(request)]))
