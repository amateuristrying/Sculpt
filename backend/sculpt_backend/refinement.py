"""Engine-independent conservative mesh processing and bounded user settings."""
import math

import numpy as np

DEFAULTS = dict(resolution=128, densityThreshold=25.0,
                removeSmallComponents=False, smoothingIterations=0)
MIN_COMPONENT_AREA_FRACTION = 0.005


def check_refinement_memory(resolution, available_gb):
    # Cached extraction loads only the small decoder, not the image encoder or
    # transformer. Reserve headroom for the cubic density grid and MPS copies.
    needed = 3.0 if resolution > 192 else 2.0 if resolution > 128 else 1.5
    if available_gb < needed:
        raise ValueError(f'Only {available_gb:.1f} GB of memory is available; refinement at {resolution} needs {needed:.1f} GB of headroom. Close other apps or lower the extraction resolution.')


def refinement_settings(value=None, default_resolution=128):
    if value is None:
        value = {}
    if not isinstance(value, dict) or set(value) - set(DEFAULTS):
        raise ValueError('Unknown mesh refinement settings.')
    settings = {**DEFAULTS, 'resolution': default_resolution, **value}
    resolution, threshold = settings['resolution'], settings['densityThreshold']
    iterations = settings['smoothingIterations']
    if type(resolution) is not int or not 96 <= resolution <= 256:
        raise ValueError('Extraction resolution must be an integer from 96 to 256.')
    if (type(threshold) not in {int, float} or not math.isfinite(threshold)
            or not 10 <= threshold <= 40):
        raise ValueError('Density threshold must be between 10 and 40.')
    if type(iterations) is not int or not 0 <= iterations <= 10:
        raise ValueError('Smoothing must use between 0 and 10 iterations.')
    if type(settings['removeSmallComponents']) is not bool:
        raise ValueError('Remove small components must be enabled or disabled.')
    return settings


def clean_mesh(mesh, settings):
    """Return a separate mesh; never mutate the source or remove attached detail."""
    from trimesh.smoothing import filter_taubin
    import trimesh

    mesh = mesh.copy()
    original_faces = len(mesh.faces)
    if not len(mesh.faces) or not len(mesh.vertices) or not np.isfinite(mesh.vertices).all():
        raise ValueError('The mesh is empty or contains invalid coordinates.')
    components_removed = 0
    if settings['removeSmallComponents']:
        parts = list(mesh.split(only_watertight=False))
        if parts:
            largest = max(range(len(parts)), key=lambda index: parts[index].area)
            minimum_area = sum(part.area for part in parts) * MIN_COMPONENT_AREA_FRACTION
            kept = [part for index, part in enumerate(parts) if index == largest or part.area >= minimum_area]
            components_removed = len(parts) - len(kept)
            mesh = trimesh.util.concatenate(kept)
    if settings['smoothingIterations']:
        # Each user iteration is a shrink/inflate pair, limiting volume loss.
        filter_taubin(mesh, lamb=0.5, nu=0.53, iterations=settings['smoothingIterations'] * 2)
    if not np.isfinite(mesh.vertices).all():
        raise ValueError('Mesh refinement produced invalid coordinates.')
    return mesh, dict(componentsRemoved=components_removed,
                      facesRemoved=original_faces - len(mesh.faces),
                      smoothingIterations=settings['smoothingIterations'])
