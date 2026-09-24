"""A bounded, non-pickle interchange format for TripoSR's reusable scene code."""
import io
import json
from pathlib import Path
import re
import zipfile

import numpy as np

from .config import MODEL_REVISION, UPSTREAM_REVISION
from .files import atomic_write_bytes

CACHE_FILENAME = 'scene-cache.npz'
SCENE_SHAPE = (1, 3, 40, 64, 64)
MAX_CACHE_BYTES = 2 * 1024 * 1024


def validate_source_hash(value):
    if not isinstance(value, str) or re.fullmatch(r'[0-9a-f]{64}', value) is None:
        raise ValueError('A valid source SHA-256 is required for the scene cache.')
    return value


def _validate_scene(scene):
    if scene.shape != SCENE_SHAPE or scene.dtype != np.dtype('float32'):
        raise ValueError('The scene cache has an unsupported shape or numeric type.')
    if not np.isfinite(scene).all():
        raise ValueError('The scene cache contains invalid coordinates.')


def write_scene_cache(path: Path, scene, source_sha256: str, background: str, mask_sha256: str | None = None):
    _validate_scene(scene)
    validate_source_hash(source_sha256)
    if background not in {'auto', 'keep'}:
        raise ValueError('Unknown background mode in scene cache.')
    metadata = dict(version=1, engine='triposr', modelRevision=MODEL_REVISION,
                    sourceRevision=UPSTREAM_REVISION, sourceSha256=source_sha256,
                    background=background)
    if mask_sha256 is not None:
        metadata['maskSha256'] = validate_source_hash(mask_sha256)
    encoded = np.frombuffer(json.dumps(metadata, sort_keys=True).encode('utf-8'), dtype=np.uint8)
    with io.BytesIO() as buffer:
        # No pickle, compression bomb, tensor device, or executable class state.
        np.savez(buffer, scene_codes=scene, metadata=encoded)
        data = buffer.getvalue()
    if len(data) > MAX_CACHE_BYTES:
        raise ValueError('The scene cache exceeds its size limit.')
    atomic_write_bytes(path, data)


def read_scene_cache(path: Path, source_sha256: str):
    validate_source_hash(source_sha256)
    path = Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_CACHE_BYTES:
        raise ValueError('The saved scene is missing or exceeds its size limit. Generate a new asset.')
    try:
        # Inspect archive members before NumPy allocates their declared arrays.
        data = path.read_bytes()
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            entries = archive.infolist()
            if (len(entries) != 2 or {entry.filename for entry in entries} != {'scene_codes.npy', 'metadata.npy'}
                    or any(entry.compress_type != zipfile.ZIP_STORED for entry in entries)
                    or sum(entry.file_size for entry in entries) > MAX_CACHE_BYTES):
                raise ValueError('Invalid scene cache archive.')
            # A forged NPY header can claim a huge array despite a tiny ZIP entry.
            for entry in entries:
                with archive.open(entry) as stream:
                    version = np.lib.format.read_magic(stream)
                    if version != (1, 0):
                        raise ValueError('Unsupported scene cache array version.')
                    shape, fortran, dtype = np.lib.format.read_array_header_1_0(stream, max_header_size=1024)
                    if entry.filename == 'scene_codes.npy':
                        if shape != SCENE_SHAPE or dtype != np.dtype('float32') or fortran:
                            raise ValueError('Invalid scene cache array header.')
                    elif len(shape) != 1 or shape[0] > 4096 or dtype != np.dtype('uint8') or fortran:
                        raise ValueError('Invalid scene cache metadata header.')
        with np.load(io.BytesIO(data), allow_pickle=False, max_header_size=1024) as archive:
            scene = archive['scene_codes']
            metadata = json.loads(archive['metadata'].tobytes().decode('utf-8'))
        _validate_scene(scene)
        if (not isinstance(metadata, dict) or metadata.get('version') != 1
                or metadata.get('engine') != 'triposr' or metadata.get('modelRevision') != MODEL_REVISION
                or metadata.get('sourceRevision') != UPSTREAM_REVISION
                or metadata.get('sourceSha256') != source_sha256
                or metadata.get('background') not in {'auto', 'keep'}):
            raise ValueError('The saved scene does not match this image or installed engine. Generate a new asset.')
        if metadata.get('maskSha256') is not None:
            validate_source_hash(metadata['maskSha256'])
        return scene, metadata
    except (OSError, ValueError, KeyError, EOFError, zipfile.BadZipFile) as error:
        raise ValueError(f'Cannot read the saved scene: {error}') from error
