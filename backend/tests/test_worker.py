import json
import os
from pathlib import Path
import subprocess
import sys
import signal
import selectors

import pytest


WORKER = Path(__file__).resolve().parents[1] / "worker.py"


def test_missing_runtime_returns_protocol_error_and_no_asset(tmp_path):
    request = tmp_path / "request.json"
    request.write_text('{}')
    result = subprocess.run([sys.executable, str(WORKER), "--request", str(request)], capture_output=True, text=True,
                            env={**os.environ, "SCULPT_RUNTIME_DIR": str(tmp_path)}, timeout=10)
    assert result.returncode == 1
    event = json.loads(result.stdout)
    assert event["protocol"] == 1 and event["type"] == "error"
    assert "not installed" in event["message"]
    assert not list(tmp_path.glob('*.glb'))


def test_worker_exits_if_desktop_died_before_interpreter_started(tmp_path):
    result = subprocess.run(
        [sys.executable, str(WORKER), '--request', str(tmp_path / 'missing.json')],
        capture_output=True, text=True, timeout=5,
        env={**os.environ, 'SCULPT_PARENT_PID': '0'},
    )
    assert result.returncode == 130
    # It must stop before runtime probing, imports or any reconstruction work.
    assert result.stdout == '' and result.stderr == ''


@pytest.mark.skipif(os.name != 'posix', reason='Desktop worker uses a POSIX process group')
def test_desktop_death_stops_worker_and_its_descendants():
    child_code = '''
import os, subprocess, sys, time
from worker import start_parent_watchdog
start_parent_watchdog()
subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])
print(os.getpid(), flush=True)
time.sleep(30)
'''
    parent_code = f'''
import os, subprocess, sys, time
subprocess.Popen([sys.executable, '-c', {child_code!r}],
                 start_new_session=True,
                 env={{**os.environ, 'SCULPT_PARENT_PID': str(os.getpid())}})
time.sleep(30)
'''
    # Both descendants inherit this pipe. EOF proves the watchdog terminates
    # the complete group, including a child that outlives its Python parent.
    parent = subprocess.Popen(
        [sys.executable, '-c', parent_code], cwd=WORKER.parent,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    worker_pid = None
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(parent.stdout, selectors.EVENT_READ)
            assert selector.select(timeout=5), 'Worker did not start'
        worker_pid = int(parent.stdout.readline())
        parent.terminate()
        parent.wait(timeout=5)
        stdout, stderr = parent.communicate(timeout=5)
        assert stdout == '' and stderr == ''
    finally:
        if parent.poll() is None:
            parent.kill()
        parent.wait(timeout=5)
        if worker_pid is not None:
            try:
                os.killpg(worker_pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
