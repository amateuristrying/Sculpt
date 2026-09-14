"""CPU surface extraction compatibility for upstream torchmcubes calls.

The neural network still runs in PyTorch on MPS/CPU. Only marching cubes uses
scikit-image, avoiding a CUDA-specific compiled extension on Apple Silicon.
"""
import sys
import types


def marching_cubes(level, threshold):
    import numpy as np
    import torch
    from skimage.measure import marching_cubes as extract

    volume = level.detach().float().cpu().numpy()
    if not np.isfinite(volume).all():
        raise ValueError("The model produced non-finite density values.")
    if not float(volume.min()) < threshold < float(volume.max()):
        raise ValueError("No surface was reconstructed. Try one clearly visible object.")
    vertices, faces, _, _ = extract(volume, threshold, allow_degenerate=False)
    # torchmcubes returns xyz in reverse grid-axis order; upstream reverses it
    # back. Preserve that public operator contract, including outward winding.
    return (
        torch.from_numpy(np.ascontiguousarray(vertices[:, ::-1])),
        torch.from_numpy(np.ascontiguousarray(faces[:, ::-1].astype(np.int64))),
    )


def install_cpu_operator() -> None:
    module = types.ModuleType("torchmcubes")
    module.marching_cubes = marching_cubes
    sys.modules["torchmcubes"] = module
