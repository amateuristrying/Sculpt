import io
import json
import zipfile

import numpy as np
import pytest

from sculpt_backend.scene_cache import (CACHE_FILENAME, SCENE_SHAPE, MAX_CACHE_BYTES,
                                        read_scene_cache, write_scene_cache)

SOURCE_SHA = 'a' * 64


def test_scene_cache_roundtrip_preserves_numeric_scene_and_source_identity(tmp_path):
    scene = np.random.default_rng(4).normal(size=SCENE_SHAPE).astype(np.float32)
    path = tmp_path / CACHE_FILENAME
    write_scene_cache(path, scene, SOURCE_SHA, 'auto')
    restored, metadata = read_scene_cache(path, SOURCE_SHA)
    assert np.array_equal(restored, scene)
    assert restored.dtype == np.float32
    assert metadata['sourceSha256'] == SOURCE_SHA
    assert metadata['background'] == 'auto'
    assert path.stat().st_size < MAX_CACHE_BYTES


@pytest.mark.parametrize('case', ['source', 'revision', 'nan', 'shape', 'pickle', 'compressed', 'truncated', 'huge_header'])
def test_scene_cache_rejects_invalid_or_incompatible_data_before_inference(tmp_path, case):
    path = tmp_path / CACHE_FILENAME
    scene = np.zeros(SCENE_SHAPE, dtype=np.float32)
    write_scene_cache(path, scene, SOURCE_SHA, 'auto')
    _, metadata = read_scene_cache(path, SOURCE_SHA)
    if case == 'source':
        metadata['sourceSha256'] = 'b' * 64
    elif case == 'revision':
        metadata['modelRevision'] = 'old model'
    elif case == 'nan':
        scene.flat[0] = np.nan
    elif case == 'shape':
        scene = np.zeros((1, 3, 40, 32, 32), dtype=np.float32)
    elif case == 'pickle':
        scene = np.array([{'unsafe': 'object'}], dtype=object)
    encoded = np.frombuffer(json.dumps(metadata).encode(), dtype=np.uint8)
    if case == 'compressed':
        np.savez_compressed(path, scene_codes=scene, metadata=encoded)
    elif case == 'truncated':
        path.write_bytes(path.read_bytes()[:256])
    elif case == 'huge_header':
        # No huge allocation is made in this test; only a forged shape header.
        with io.BytesIO() as array:
            np.lib.format.write_array_header_1_0(array, dict(descr='<f4', fortran_order=False, shape=(2**50,)))
            with zipfile.ZipFile(path, 'w') as archive:
                archive.writestr('scene_codes.npy', array.getvalue())
                archive.writestr('metadata.npy', b'')
    else:
        np.savez(path, scene_codes=scene, metadata=encoded)
    with pytest.raises(ValueError, match='saved scene'):
        read_scene_cache(path, SOURCE_SHA)


def test_scene_cache_rejects_invalid_hash_and_does_not_write(tmp_path):
    with pytest.raises(ValueError, match='SHA-256'):
        write_scene_cache(tmp_path / CACHE_FILENAME, np.zeros(SCENE_SHAPE, np.float32), 'wrong', 'auto')
    assert not list(tmp_path.iterdir())


def test_scene_cache_rejects_oversize_and_symlink(tmp_path):
    source = tmp_path / 'source.npz'
    source.write_bytes(b'\0' * (MAX_CACHE_BYTES + 1))
    with pytest.raises(ValueError, match='size limit'):
        read_scene_cache(source, SOURCE_SHA)
    link = tmp_path / 'linked.npz'
    link.symlink_to(source)
    with pytest.raises(ValueError, match='size limit'):
        read_scene_cache(link, SOURCE_SHA)


def test_mask_provenance_survives_cached_refinement(tmp_path):
    import numpy as np
    from sculpt_backend.scene_cache import write_scene_cache, read_scene_cache, SCENE_SHAPE
    path = tmp_path / 'scene.npz'
    write_scene_cache(path, np.zeros(SCENE_SHAPE, dtype=np.float32), 'a' * 64, 'auto', 'b' * 64)
    _, metadata = read_scene_cache(path, 'a' * 64)
    assert metadata['maskSha256'] == 'b' * 64
