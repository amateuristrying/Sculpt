import types

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
    image = _rasterize(mesh, 32)
    assert isinstance(image, Image.Image)
    pixels = np.asarray(image)
    assert pixels.reshape(-1, 3).max(axis=0).min() > 20
    assert np.count_nonzero(pixels.sum(axis=2)) > 100


def test_baked_glb_contains_embedded_color_texture_and_reopens(tmp_path):
    mesh = trimesh.creation.icosphere(subdivisions=2)
    mesh.visual.vertex_colors = np.tile([210, 100, 35, 255], (len(mesh.vertices), 1))
    textured, image, metrics = bake(mesh, 1024, 256)
    assert metrics['textureResolution'] == '1K'
    assert metrics['facesAfter'] <= metrics['facesBefore']
    output = tmp_path / 'textured.glb'
    export_textured_glb(textured, image, output)
    scene = trimesh.load(output, force='scene', process=False)
    loaded = next(iter(scene.geometry.values()))
    assert loaded.visual.material.baseColorTexture.size == (1024, 1024)
    assert loaded.visual.uv.shape[0] == len(loaded.vertices)
