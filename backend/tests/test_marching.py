import numpy as np
import pytest
import torch
import trimesh
from sculpt_backend.marching import marching_cubes


def test_asymmetric_surface_axes_and_outward_winding():
    grid = np.stack(np.meshgrid(*[np.arange(32)] * 3, indexing="ij"), -1)
    center = np.array([9., 14., 20.])
    density = 5 - np.linalg.norm(grid - center, axis=-1)
    vertices, faces = marching_cubes(torch.tensor(density), 0)
    # Upstream consumes reversed axes, then normalizes coordinates.
    mesh = trimesh.Trimesh(vertices=vertices.numpy()[:, [2, 1, 0]], faces=faces.numpy(), process=False)
    assert np.allclose(mesh.bounding_box.centroid, center, atol=.1)
    assert mesh.is_watertight
    assert mesh.volume > 0, "Reconstruction must have outward-facing triangles"


@pytest.mark.parametrize("value", [0., float("nan"), float("inf")])
def test_invalid_or_empty_density_is_not_a_fake_asset(value):
    with pytest.raises(ValueError):
        marching_cubes(torch.full((8, 8, 8), value), .5)
