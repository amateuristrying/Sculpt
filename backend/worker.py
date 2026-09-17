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

from sculpt_backend import PROTOCOL_VERSION
from sculpt_backend.config import configure_environment, runtime_root
from sculpt_backend.health import validate_install


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

    def message(kind, **fields):
        print(json.dumps({"protocol": PROTOCOL_VERSION, "type": kind, **fields}), file=protocol_stdout, flush=True)

    root = runtime_root()
    configure_environment(root)
    try:
        with contextlib.redirect_stdout(sys.stderr):
            if args.probe:
                import torch
                import rembg
                import skimage
                installed = validate_install(root)
                message("probe", installed=installed, mps=torch.backends.mps.is_available(), torchVersion=torch.__version__)
                return 0
            if args.request is None:
                raise ValueError("A request file is required.")
            request = json.loads(args.request.read_text())
            if not validate_install(root):
                raise ValueError("The local engine is not installed or needs repair. Open runtime setup in Sculpt.")
            from sculpt_backend.triposr import generate
            result = generate(request, root, lambda stage, progress, text: message("progress", stage=stage, progress=progress, message=text))
            message("result", metrics=result)
        return 0
    except Exception as error:
        traceback.print_exc(file=sys.stderr)
        message("error", message=f"{type(error).__name__}: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
