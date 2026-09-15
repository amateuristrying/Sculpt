import json
import struct
import pytest
import trimesh
from benchmark import validate_glb


def test_independently_reads_geometry(tmp_path):
    file = tmp_path / 'mesh.glb'
    trimesh.creation.box().export(file)
    assert validate_glb(file) == {'faces': 12, 'vertices': 8}


def test_rejects_truncated_result(tmp_path):
    file = tmp_path / 'mesh.glb'
    file.write_bytes(b'glTF')
    with pytest.raises(ValueError, match='header'):
        validate_glb(file)


def test_never_follows_external_uris(tmp_path):
    payload = json.dumps({'buffers': [{'uri': 'https://example.invalid/mesh.bin'}]}).encode()
    data = b'glTF' + struct.pack('<IIII', 2, 20 + len(payload), len(payload), 0x4E4F534A) + payload
    file = tmp_path / 'mesh.glb'
    file.write_bytes(data)
    with pytest.raises(ValueError, match='self-contained'):
        validate_glb(file)
