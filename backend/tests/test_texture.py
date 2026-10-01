import numpy as np
import pytest
import trimesh
from PIL import Image

from sculpt_backend.export import export_textured_glb
from sculpt_backend.texture import _rasterize, bake, validate_texture_settings


def test_texture_settings_are_bounded_and_default_is_vertex():
    assert validate_texture_settings('vertex', None) == ('vertex', None)
    assert validate_texture_settings('1k', 20000) == ('1k', 20000)
    with pytest.raises(ValueError):
        validate_texture_settings('4k', None)
    with pytest.raises(ValueError):
        validate_texture_settings('2k', 100)


def test_cpu_rasterizer_fills_uv_triangle_without_black_interior():
    mesh = trimesh.Trimesh(
        vertices=[[0, 0, 0], [1, 0, 0], [0, 1, 0]],
        faces=[[0, 1, 2]],
        process=False,
    )
    mesh.visual.vertex_colors = np.array(
        [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]], dtype=np.uint8
    )
    mesh.visual.uv = np.array([[0.05, 0.05], [0.95, 0.05], [0.05, 0.95]], dtype=np.float32)
    image = _rasterize(mesh, 32, lambda p: np.column_stack((1 - p[:, 0] - p[:, 1], p[:, 0], p[:, 1])))
    assert isinstance(image, Image.Image)
    pixels = np.asarray(image)
    assert pixels.reshape(-1, 3).max(axis=0).min() > 20
    assert np.count_nonzero(pixels.sum(axis=2)) > 100


def test_baked_glb_contains_embedded_color_texture_and_reopens(tmp_path):
    mesh = trimesh.creation.icosphere(subdivisions=2)
    mesh.visual.vertex_colors = np.tile([210, 100, 35, 255], (len(mesh.vertices), 1))
    textured, image, metrics = bake(mesh, 1024, 256, lambda p: np.tile([.8, .4, .1], (len(p), 1)))
    assert metrics['textureResolution'] == '1K'
    assert metrics['facesAfter'] <= metrics['facesBefore']
    output = tmp_path / 'textured.glb'
    export_textured_glb(textured, image, output)
    scene = trimesh.load(output, force='scene', process=False)
    loaded = next(iter(scene.geometry.values()))
    assert loaded.visual.material.baseColorTexture.size == (1024, 1024)
    assert loaded.visual.uv.shape[0] == len(loaded.vertices)


def test_bake_queries_surface_in_bounded_batches_and_preserves_uv_orientation():
    from sculpt_backend.texture import QUERY_BATCH_SIZE
    mesh = trimesh.Trimesh(vertices=[[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]],
                           faces=[[0, 1, 2], [0, 2, 3]], process=False)
    mesh.visual = trimesh.visual.texture.TextureVisuals(uv=np.asarray(mesh.vertices)[:, :2])
    counts = []
    def query(points):
        counts.append(len(points))
        # Detail is deliberately absent from vertex colors. It comes from the field.
        return np.column_stack((points[:, 0] ** 2, points[:, 1], np.full(len(points), .05)))
    pixels = np.asarray(_rasterize(mesh, 256, query))
    assert sum(counts) == 256 ** 2
    assert max(counts) <= QUERY_BATCH_SIZE
    assert len(counts) > 1
    assert pixels[0, 0, 1] > 250  # high V at TOP of PNG
    assert pixels[-1, 0, 1] < 3
    assert 61 <= pixels[128, 128, 0] <= 66  # quadratic field, not linear interpolation


def test_uv_seams_retain_normals_and_topology():
    from sculpt_backend.texture import unwrap, topology
    mesh = trimesh.creation.icosphere(subdivisions=1)
    result = unwrap(mesh, 1024)
    from scipy.spatial import cKDTree
    _, indices = cKDTree(mesh.vertices).query(result.vertices)
    np.testing.assert_allclose(result.vertex_normals, mesh.vertex_normals[indices], atol=1e-6)
    _, _, metrics = bake(mesh, 1024, None, lambda p: np.full_like(p, .3))
    assert metrics['meshQuality'] == topology(mesh)
    assert metrics['meshQuality']['watertight']


def test_invalid_engine_colors_are_rejected():
    mesh = trimesh.Trimesh(vertices=[[0, 0, 0], [1, 0, 0], [0, 1, 0]], faces=[[0, 1, 2]])
    mesh.visual = trimesh.visual.texture.TextureVisuals(uv=np.asarray(mesh.vertices)[:, :2])
    with pytest.raises(ValueError, match='invalid texture colors'):
        _rasterize(mesh, 16, lambda p: np.full_like(p, np.nan))


def test_decimation_retries_and_preserves_closed_original_when_target_is_unsafe(monkeypatch):
    import fast_simplification
    from sculpt_backend.texture import reduce_mesh
    mesh = trimesh.creation.icosphere(subdivisions=2)
    calls = []
    def broken(vertices, faces, **kwargs):
        calls.append(kwargs['agg'])
        return vertices, faces[:10]  # an open patch, never an acceptable replacement
    monkeypatch.setattr(fast_simplification, 'simplify', broken)
    reduced, metrics = reduce_mesh(mesh, 256)
    assert calls == [5, 3, 1]
    assert reduced.is_watertight
    np.testing.assert_array_equal(reduced.faces, mesh.faces)
    assert metrics['reductionLimited'] and not metrics['targetReached']
