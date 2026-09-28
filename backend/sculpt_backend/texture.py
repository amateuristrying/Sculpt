"""Local UV packing and color baking shared by every inference engine.

The neural worker supplies geometry and colors. This module owns the post
processing boundary: optional face reduction, UV parametrization, and a CPU
triangle rasterizer that embeds a PNG in the resulting GLB. It deliberately
does not import a model or an OpenGL/nvdiffrast helper.
"""
from __future__ import annotations

from io import BytesIO

import numpy as np
from PIL import Image

TEXTURE_RESOLUTIONS = {"vertex": None, "1k": 1024, "2k": 2048}


def validate_texture_settings(value: str, target_faces: int | None) -> tuple[str, int | None]:
    if value not in TEXTURE_RESOLUTIONS:
        raise ValueError("Texture mode must be vertex, 1k, or 2k.")
    if target_faces is not None and (
        type(target_faces) is not int or not 256 <= target_faces <= 250_000
    ):
        raise ValueError("Texture face target must be an integer from 256 to 250000.")
    return value, target_faces


def reduce_mesh(mesh, target_faces: int | None):
    """Return a reduced copy, retaining vertex colors for the bake."""
    if target_faces is None or len(mesh.faces) <= target_faces:
        return mesh.copy(), {
            "facesBefore": int(len(mesh.faces)),
            "facesAfter": int(len(mesh.faces)),
            "reduced": False,
        }
    try:
        import fast_simplification
    except ImportError as error:
        raise ValueError(
            "Texture baking needs the optional MIT mesh simplifier. "
            "Repair the local runtime and retry."
        ) from error
    reduction = 1.0 - target_faces / len(mesh.faces)
    vertices, faces = fast_simplification.simplify(
        np.asarray(mesh.vertices, dtype=np.float32),
        np.asarray(mesh.faces, dtype=np.int32),
        target_reduction=float(min(max(reduction, 0.0), 0.98)),
    )
    import trimesh

    reduced = trimesh.Trimesh(vertices=vertices, faces=faces, process=False)
    source = np.asarray(
        getattr(mesh.visual, "vertex_colors", np.full((len(mesh.vertices), 4), 180)),
        dtype=np.uint8,
    )
    nearest = np.argmin(
        ((vertices[:, None, :] - mesh.vertices[None, :, :]) ** 2).sum(axis=2),
        axis=1,
    )
    reduced.visual.vertex_colors = source[nearest]
    return reduced, {
        "facesBefore": int(len(mesh.faces)),
        "facesAfter": int(len(reduced.faces)),
        "reduced": True,
    }


def unwrap(mesh):
    try:
        import xatlas
    except ImportError as error:
        raise ValueError(
            "Texture baking needs the optional MIT xatlas UV packer. "
            "Repair the local runtime and retry."
        ) from error
    vmapping, indices, uvs = xatlas.parametrize(
        np.asarray(mesh.vertices, dtype=np.float32),
        np.asarray(mesh.faces, dtype=np.uint32),
    )
    import trimesh

    textured = trimesh.Trimesh(
        vertices=np.asarray(mesh.vertices)[vmapping],
        faces=np.asarray(indices, dtype=np.uint32),
        process=False,
    )
    textured.visual.vertex_colors = np.asarray(mesh.visual.vertex_colors)[vmapping]
    textured.visual.uv = np.asarray(uvs, dtype=np.float32)
    return textured


def _rasterize(mesh, resolution: int) -> Image.Image:
    """Rasterize vertex colors into UV space with conservative seam padding."""
    uv = np.asarray(mesh.visual.uv, dtype=np.float64)
    colors = np.asarray(mesh.visual.vertex_colors, dtype=np.float64)[:, :3] / 255.0
    image = np.zeros((resolution, resolution, 3), dtype=np.float64)
    covered = np.zeros((resolution, resolution), dtype=bool)
    for face in np.asarray(mesh.faces, dtype=np.int64):
        points = uv[face] * (resolution - 1)
        lo = np.maximum(np.floor(points.min(axis=0)).astype(int), 0)
        hi = np.minimum(np.ceil(points.max(axis=0)).astype(int), resolution - 1)
        if np.any(lo > hi):
            continue
        a, b, c = points
        denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if abs(denominator) < 1e-10:
            continue
        x, y = np.meshgrid(
            np.arange(lo[0], hi[0] + 1) + 0.5,
            np.arange(lo[1], hi[1] + 1) + 0.5,
        )
        w0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator
        w1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator
        w2 = 1.0 - w0 - w1
        mask = (w0 >= -1e-6) & (w1 >= -1e-6) & (w2 >= -1e-6)
        patch = np.stack((w0, w1, w2), axis=-1) @ colors[face]
        y0, y1, x0, x1 = lo[1], hi[1] + 1, lo[0], hi[0] + 1
        image_patch = image[y0:y1, x0:x1]
        covered_patch = covered[y0:y1, x0:x1]
        image_patch[mask] = patch[mask]
        covered_patch[mask] = True
    # A one-pixel dilation prevents black seams in bilinear sampling.
    for _ in range(2):
        missing = ~covered
        if not missing.any():
            break
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            source_y = np.clip(np.arange(resolution)[:, None] + dy, 0, resolution - 1)
            source_x = np.clip(np.arange(resolution)[None, :] + dx, 0, resolution - 1)
            copy = missing & covered[source_y, source_x]
            image[copy] = image[source_y, source_x][copy]
            covered[copy] = True
    return Image.fromarray(np.rint(np.clip(image, 0, 1) * 255).astype(np.uint8)).convert("RGB")


def bake(mesh, resolution: int, target_faces: int | None):
    if resolution not in (1024, 2048):
        raise ValueError("Baked textures support 1K or 2K resolution.")
    simplified, metrics = reduce_mesh(mesh, target_faces)
    textured = unwrap(simplified)
    image = _rasterize(textured, resolution)
    return textured, image, {**metrics, "textureResolution": f"{resolution // 1024}K"}


def png_bytes(image: Image.Image) -> bytes:
    output = BytesIO()
    image.save(output, format="PNG", optimize=True)
    return output.getvalue()
