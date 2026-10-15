"""Independent import check for Rust's OBJ/glTF exports; no inference or downloads.

Run after the opt-in export_saved_exchange_assets Rust test. Uses the existing
locked trimesh/numpy runtime. This does not substitute for Blender import QA.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import struct
import zipfile

import numpy as np
import trimesh


def canonical_triangles(mesh):
    # Rotate each face to its lexicographically smallest vertex, preserving winding.
    triangles = mesh.triangles.copy()
    rows = []
    for triangle in triangles:
        start = min(range(3), key=lambda i: tuple(triangle[i]))
        rows.append(np.roll(triangle, -start, axis=0).reshape(-1))
    rows = np.asarray(rows)
    return rows[np.lexsort(rows.T[::-1])]


def verify(directory):
    data = (directory / 'source.glb').read_bytes()
    json_length = struct.unpack_from('<I', data, 12)[0]
    original_document = json.loads(data[20:20 + json_length])
    document = json.loads((directory / 'model.gltf').read_bytes())
    binary = base64.b64decode(document['buffers'][0].pop('uri').split(',', 1)[1])
    original_binary = data[28 + json_length:28 + json_length + len(binary)]
    assert document == original_document and binary == original_binary
    with zipfile.ZipFile(directory / 'model.zip') as archive:
        assert archive.testzip() is None
        # The native exporter uses fixed names only; never extract arbitrary paths.
        for entry in archive.namelist():
            assert Path(entry).name == entry and entry not in ('.', '..')
            (directory / entry).write_bytes(archive.read(entry))
        for i, image in enumerate(document.get('images', [])):
            view = document['bufferViews'][image['bufferView']]
            start = view.get('byteOffset', 0)
            assert archive.read(f'texture_{i}.png') == binary[start:start + view['byteLength']]

    source = trimesh.load(directory / 'source.glb', force='mesh', process=False)
    results = {}
    for extension in ['gltf', 'obj']:
        mesh = trimesh.load(directory / f'model.{extension}', force='mesh', process=False, maintain_order=True)
        expected, actual = canonical_triangles(source), canonical_triangles(mesh)
        assert actual.shape == expected.shape
        np.testing.assert_allclose(actual, expected, atol=1e-7, rtol=0)
        original, loaded = source.copy(), mesh.copy()
        for candidate in [original, loaded]:
            candidate.merge_vertices(merge_tex=True, merge_norm=True)
        assert original.is_watertight == loaded.is_watertight
        assert original.is_winding_consistent == loaded.is_winding_consistent
        if extension == 'obj' and source.visual.kind == 'texture' and source.visual.uv is not None:
            np.testing.assert_allclose(mesh.visual.uv, source.visual.uv, atol=1e-7)
            np.testing.assert_array_equal(np.asarray(mesh.visual.material.image), np.asarray(source.visual.material.baseColorTexture))
        results[extension] = {'faces': len(mesh.faces), 'max_position_error': float(np.max(np.abs(actual - expected))),
                              'watertight_after_seam_weld': bool(loaded.is_watertight), 'bytes': (directory / f'model.{"zip" if extension == "obj" else extension}').stat().st_size}
    return {'case': directory.name, 'source_sha256': hashlib.sha256(data).hexdigest(),
            'source_bytes': len(data), 'exports': results}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    arguments = parser.parse_args()
    print(json.dumps([verify(case) for case in sorted(arguments.directory.iterdir()) if (case / 'source.glb').is_file()], indent=2))
