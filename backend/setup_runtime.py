"""Installer used by both Tauri and the developer CLI. No shell or global pip."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import threading
import time
import signal
import urllib.request

from sculpt_backend.config import SPEC, UPSTREAM_REVISION, runtime_root
from sculpt_backend.health import checksum
from sculpt_backend.protocol import progress, emit


def python_executable(root: Path) -> Path:
    """Resolve the python executable path in the virtual environment across platforms."""
    if os.name == 'nt':
        scripts_py = root / 'venv' / 'Scripts' / 'python.exe'
        if scripts_py.exists():
            return scripts_py
        venv_py = root / 'venv' / 'python.exe'
        if venv_py.exists():
            return venv_py
        return scripts_py
    return root / 'venv' / 'bin' / 'python'


def install_source(root):
    source = root / 'TripoSR'
    archive = root / 'source.tar.gz'
    if not archive.is_file() or checksum(archive) != SPEC['sourceArchiveSha256']:
        with urllib.request.urlopen(f'https://codeload.github.com/VAST-AI-Research/TripoSR/tar.gz/{UPSTREAM_REVISION}', timeout=90) as response, archive.open('wb') as output:
            shutil.copyfileobj(response, output)
    if checksum(archive) != SPEC['sourceArchiveSha256']:
        if archive.exists():
            archive.unlink()
        raise ValueError('The engine source download failed its checksum. Retry setup.')
    with tarfile.open(archive, 'r:gz') as packed:
        prefix = f'TripoSR-{UPSTREAM_REVISION}/'
        for member in packed.getmembers():
            if not member.isfile() or not member.name.startswith(prefix):
                continue
            # Path traversal validation
            member_path = Path(member.name)
            if member_path.is_absolute() or '..' in member_path.parts:
                raise ValueError(f'Unsafe engine archive path: {member.name}')
            relative = Path(member.name[len(prefix):])
            if relative.is_absolute() or '..' in relative.parts:
                raise ValueError(f'Unsafe engine archive path: {member.name}')
            target = (source / relative).resolve()
            source_resolved = source.resolve()
            if not target.is_relative_to(source_resolved):
                raise ValueError(f'Unsafe engine archive path escapes source directory: {member.name}')
            target.parent.mkdir(parents=True, exist_ok=True)
            with packed.extractfile(member) as data, target.open('wb') as output:
                shutil.copyfileobj(data, output)


def main():
    if os.environ.get('SCULPT_PARENT_PID'):
        parent_pid = int(os.environ['SCULPT_PARENT_PID'])
        def watch_app():
            while True:
                try:
                    os.kill(parent_pid, 0)
                except (ProcessLookupError, OSError):
                    if hasattr(os, 'killpg') and hasattr(os, 'getpgrp'):
                        try:
                            os.killpg(os.getpgrp(), signal.SIGKILL)
                        except OSError:
                            pass
                    else:
                        try:
                            os.kill(os.getpid(), signal.SIGTERM)
                        except OSError:
                            pass
                    os._exit(1)
                time.sleep(1)
        threading.Thread(target=watch_app, daemon=True).start()
    root = runtime_root()
    backend = Path(__file__).resolve().parent
    uv = os.environ.get('SCULPT_UV') or shutil.which('uv')
    if not uv:
        raise ValueError('Open runtime setup in Sculpt, or install uv before using this developer command.')
    root.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(root).free < 5 * 1024**3:
        raise ValueError('At least 5 GB free disk space is required for runtime setup.')
    progress('python', 12, 'Preparing an isolated Python environment')
    python = python_executable(root)
    if not python.exists():
        subprocess.run([uv, 'venv', '--python', '3.11', str(root / 'venv')], check=True, stdout=sys.stderr)
    python = python_executable(root)
    if not python.exists():
        raise RuntimeError(f'Python executable not found after venv creation: {python}')
    progress('dependencies', 25, 'Installing verified runtime dependencies')
    subprocess.run([uv, 'pip', 'install', '--python', str(python), '--require-hashes', '-r', str(backend / 'requirements.lock')], check=True, stdout=sys.stderr)
    progress('engine', 42, 'Preparing the pinned reconstruction engine')
    install_source(root)
    subprocess.run([str(python), str(backend / 'download_models.py')], check=True, env={**os.environ, 'SCULPT_RUNTIME_DIR': str(root)})


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        emit('error', message=str(error))
        raise SystemExit(1)
