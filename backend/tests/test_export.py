import json
import struct
import numpy as np
import trimesh
from sculpt_backend.export import export_glb, linear_vertex_colors


def test_export_preserves_geometry_colors_and_explicit_nonmetallic_material(tmp_path):
    mesh = trimesh.creation.icosphere(subdivisions=1)
    mesh.visual.vertex_colors = np.tile([240, 180, 20, 255], (len(mesh.vertices), 1))
    output = tmp_path / 'mesh.glb'
    export_glb(mesh, output)
    data = output.read_bytes()
    document = json.loads(data[20:20 + struct.unpack_from('<I', data, 12)[0]])
    assert document['materials'][0]['pbrMetallicRoughness']['metallicFactor'] == 0
    assert document['materials'][0]['pbrMetallicRoughness']['roughnessFactor'] == 0.8
    primitive = document['meshes'][0]['primitives'][0]
    assert 'COLOR_0' in primitive['attributes'] and 'NORMAL' in primitive['attributes']
    loaded = trimesh.load(output, force='mesh', process=False)
    np.testing.assert_allclose(loaded.vertices, mesh.vertices, atol=1e-7)
    np.testing.assert_array_equal(loaded.faces, mesh.faces)
    assert document['accessors'][primitive['attributes']['COLOR_0']]['count'] == len(mesh.vertices)


def test_color_conversion_preserves_alpha_and_corrects_srgb_midtones():
    samples = np.array([[0, 128, 255, 173]], dtype=np.uint8)
    np.testing.assert_array_equal(linear_vertex_colors(samples), [[0, 55, 255, 173]])
    np.testing.assert_array_equal(samples, [[0, 128, 255, 173]])
