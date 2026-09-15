"""Checksums at install/repair, file identity before every offline job."""
import hashlib
import json
from pathlib import Path
from .config import SPEC

LOCKFILE = Path(__file__).resolve().parents[1] / 'requirements.lock'


def checksum(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def validate_install(root):
    try:
        manifest = json.loads((root / 'ready.json').read_text())
        if manifest.get('version') != SPEC['version'] or manifest.get('modelRevision') != SPEC['modelRevision'] or manifest.get('sourceRevision') != SPEC['sourceRevision']:
            return False
        if manifest.get('dependencyLockSha256') != checksum(LOCKFILE):
            return False
        files = manifest['files']
        for required in [*SPEC['artifacts'], 'TripoSR/tsr/system.py', 'venv/bin/python']:
            if required not in files:
                return False
        for name, expected in files.items():
            path = root / name
            if Path(name).is_absolute() or '..' in Path(name).parts:
                return False
            stat = path.stat()
            if stat.st_size != expected['size'] or stat.st_mtime_ns != expected['modifiedNs']:
                return False
        return True
    except (OSError, ValueError, KeyError, TypeError):
        return False


def write_verified_manifest(root, extra_files=()):
    import torch
    import psutil
    import rembg
    import skimage
    files = {}
    for name, expected in SPEC['artifacts'].items():
        if checksum(root / name) != expected:
            raise ValueError(f'Checksum mismatch for {name}. Remove the damaged file and retry setup.')
    names = [*SPEC['artifacts'], 'venv/bin/python', *extra_files]
    names += [str(Path(module.__file__).relative_to(root)) for module in (torch, psutil, rembg, skimage)]
    names += [str(path.relative_to(root)) for path in (root / 'TripoSR' / 'tsr').rglob('*.py')]
    for name in names:
        stat = (root / name).stat()
        files[name] = {'size': stat.st_size, 'modifiedNs': stat.st_mtime_ns}
    manifest = {'version': SPEC['version'], 'engine': 'triposr', 'modelRevision': SPEC['modelRevision'],
                'sourceRevision': SPEC['sourceRevision'], 'torchVersion': torch.__version__,
                'mpsAvailable': torch.backends.mps.is_available(), 'files': files,
                'dependencyLockSha256': checksum(LOCKFILE)}
    temporary = root / 'ready.json.tmp'
    temporary.write_text(json.dumps(manifest, indent=2))
    temporary.replace(root / 'ready.json')
