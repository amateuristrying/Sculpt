import json
import os
from pathlib import Path
import subprocess
import sys


def test_missing_runtime_returns_protocol_error_and_no_asset(tmp_path):
    request = tmp_path / "request.json"
    request.write_text('{}')
    worker = Path(__file__).resolve().parents[1] / "worker.py"
    result = subprocess.run([sys.executable, str(worker), "--request", str(request)], capture_output=True, text=True,
                            env={**os.environ, "SCULPT_RUNTIME_DIR": str(tmp_path)}, timeout=10)
    assert result.returncode == 1
    event = json.loads(result.stdout)
    assert event["protocol"] == 1 and event["type"] == "error"
    assert "not installed" in event["message"]
    assert not list(tmp_path.glob('*.glb'))
