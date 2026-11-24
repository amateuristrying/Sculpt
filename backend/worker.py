"""One isolated job per process. NDJSON stdout is a versioned Rust/worker protocol.

All library output goes to stderr. No server, socket, account, or cloud inference.
Rust owns cancellation by terminating this child and discarding incomplete output.
"""
import argparse
import contextlib
import json
from pathlib import Path
import sys
import traceback
import os
import signal
import threading
import time

from sculpt_backend.config import configure_environment, runtime_root
from sculpt_backend.health import validate_install
from sculpt_backend.protocol import (
    PROTOCOL_VERSION,
    emit,
    emit_accepted,
    emit_error,
    emit_progress,
    emit_result,
)


def start_parent_watchdog() -> None:
    # Rust supplies the expected PID before spawning. Capturing only getppid()
    # here misses a GUI crash that happens while the interpreter is starting.
    parent_pid = int(os.environ.get("SCULPT_PARENT_PID", os.getppid()))

    def check_parent():
        if os.getppid() != parent_pid:
            # Rust gives each worker its own group. Release any descendants as
            # well, but never signal the caller's terminal group for CLI usage.
            if os.name == "posix" and os.getpgrp() == os.getpid():
                os.killpg(os.getpgrp(), signal.SIGKILL)
            os._exit(130)

    check_parent()

    def watch_parent():
        while True:
            time.sleep(0.5)
            check_parent()

    threading.Thread(target=watch_parent, daemon=True).start()


def main() -> int:
    # A GUI quit or crash must not leave an orphan holding unified memory.
    start_parent_watchdog()
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", type=Path)
    parser.add_argument("--probe", action="store_true")
    args = parser.parse_args()
    protocol_stdout = sys.stdout

    root = runtime_root()
    configure_environment(root)
    job_id = None
    try:
        with contextlib.redirect_stdout(sys.stderr):
            if args.probe:
                import torch
                import rembg
                import skimage
                installed = validate_install(root)
                emit("probe", job_id=None, stream=protocol_stdout, installed=installed, mps=torch.backends.mps.is_available(), torchVersion=torch.__version__)
                return 0
            if args.request is None:
                raise ValueError("A request file is required.")
            request = json.loads(args.request.read_text())
            job_id = request.get("jobId") or request.get("job_id")
            emit_accepted(job_id=job_id, stream=protocol_stdout)
            if not validate_install(root):
                raise ValueError("The local engine is not installed or needs repair. Open runtime setup in Sculpt.")
            emit = lambda stage, progress, text: emit_progress(stage=stage, progress=progress, message=text, job_id=job_id, stream=protocol_stdout)
            if request.get('operation') == 'mask':
                from sculpt_backend.masks import prepare_mask
                result = prepare_mask(request, emit)
            else:
                from sculpt_backend.triposr import generate
                result = generate(request, root, emit)
            output_path = request.get("outputPath")
            artifacts = [output_path] if output_path else []
            emit_result(metrics=result, artifacts=artifacts, job_id=job_id, stream=protocol_stdout)
        return 0
    except Exception as error:
        traceback.print_exc(file=sys.stderr)
        emit_error(message=f"{type(error).__name__}: {error}", code="WORKER_ERROR", job_id=job_id, stream=protocol_stdout)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
