import numpy as np
import pytest
import trimesh

from sculpt_backend.refinement import clean_mesh, refinement_settings, check_refinement_memory


@pytest.mark.parametrize('settings', [
    {'resolution': 95}, {'resolution': 257}, {'resolution': True},
    {'densityThreshold': float('nan')}, {'densityThreshold': 0}, {'densityThreshold': 41},
    {'smoothingIterations': -1}, {'smoothingIterations': 11}, {'smoothingIterations': 1.5},
    {'removeSmallComponents': 1}, {'unrecognized': True}, [],
])
def test_refinement_rejects_unbounded_or_unknown_settings(settings):
    with pytest.raises(ValueError):
        refinement_settings(settings)


def test_cleanup_drops_specks_preserves_thin_parts_and_original_mesh():
    body = trimesh.creation.icosphere(subdivisions=2)
    leg = trimesh.creation.cylinder(radius=0.035, height=1.5, sections=16)
    leg.apply_translation([2, 0, 0])
    speck = trimesh.creation.icosphere(subdivisions=1, radius=0.01)
    speck.apply_translation([3, 0, 0])
    body.visual.vertex_colors = [255, 220, 0, 255]
    leg.visual.vertex_colors = [120, 80, 30, 255]
    mesh = trimesh.util.concatenate([body, leg, speck])
    original_vertices, original_faces = mesh.vertices.copy(), mesh.faces.copy()
    cleaned, metrics = clean_mesh(mesh, refinement_settings({'removeSmallComponents': True}))
    assert metrics['componentsRemoved'] == 1
    assert metrics['facesRemoved'] == len(speck.faces)
    assert len(cleaned.split(only_watertight=False)) == 2
    assert len(cleaned.faces) == len(body.faces) + len(leg.faces)
    assert np.array_equal(mesh.vertices, original_vertices) and np.array_equal(mesh.faces, original_faces)
    assert len(cleaned.visual.vertex_colors) == len(cleaned.vertices)
    assert [120, 80, 30, 255] in cleaned.visual.vertex_colors.tolist()


def test_smoothing_stays_finite_preserves_topology_and_limits_volume_loss():
    mesh = trimesh.creation.icosphere(subdivisions=3)
    vertices = mesh.vertices.copy()
    smoothed, metrics = clean_mesh(mesh, refinement_settings({'smoothingIterations': 10}))
    assert metrics['smoothingIterations'] == 10
    assert np.isfinite(smoothed.vertices).all()
    assert np.array_equal(mesh.vertices, vertices)
    assert np.array_equal(smoothed.faces, mesh.faces)
    assert smoothed.is_watertight and smoothed.is_winding_consistent
    assert 0.9 < smoothed.volume / mesh.volume < 1.1


def test_refinement_disabled_preserves_geometry_and_colors():
    mesh = trimesh.creation.icosphere(subdivisions=1)
    mesh.visual.vertex_colors = [210, 100, 20, 255]
    result, metrics = clean_mesh(mesh, refinement_settings())
    assert np.array_equal(result.vertices, mesh.vertices)
    assert np.array_equal(result.faces, mesh.faces)
    assert np.array_equal(result.visual.vertex_colors, mesh.visual.vertex_colors)
    assert metrics['componentsRemoved'] == metrics['facesRemoved'] == 0


def test_refinement_memory_accounts_for_resolution_without_full_encoder():
    check_refinement_memory(128, 1.6)
    check_refinement_memory(256, 3.1)
    with pytest.raises(ValueError, match='headroom'):
        check_refinement_memory(256, 2.9)
