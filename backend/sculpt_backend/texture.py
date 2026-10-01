"""Engine-independent UV baking. The engine supplies an sRGB color-field callback.

CPU rasterization maps texels to surface positions; bounded batches query the
cached neural scene. No OpenGL, model imports, or vertex-color resampling.
"""
from __future__ import annotations

import numpy as np
from PIL import Image

TEXTURE_RESOLUTIONS = {"vertex": None, "1k": 1024, "2k": 2048}
QUERY_BATCH_SIZE = 8192
PADDING = 8


def validate_texture_settings(value: str, target_faces: int | None) -> tuple[str, int | None]:
    if not isinstance(value, str) or value not in TEXTURE_RESOLUTIONS:
        raise ValueError("Texture mode must be vertex, 1k, or 2k.")
    if target_faces is not None and (
        type(target_faces) is not int or not 256 <= target_faces <= 250_000
    ):
        raise ValueError("Texture face target must be an integer from 256 to 250000.")
    return value, target_faces


def reduce_mesh(mesh, target_faces: int | None):
    """Reduce geometry only. Colors are queried on the resulting surface later."""
    validate_texture_settings('vertex', target_faces)
    reduced = mesh.copy()
    if target_faces is not None and len(mesh.faces) > target_faces:
        import fast_simplification
        import trimesh

        # Thin structures can produce non-manifold edges at aggressive settings.
        # A face target must never turn a closed input into an open/non-manifold
        # result. Retry conservatively; retain the original if none is suitable.
        preserve_closed = mesh.is_watertight
        for aggression in (5, 3, 1):
            vertices, faces = fast_simplification.simplify(
                np.asarray(mesh.vertices, dtype=np.float32),
                np.asarray(mesh.faces, dtype=np.int32),
                target_count=target_faces, agg=aggression,
            )
            candidate = trimesh.Trimesh(vertices=vertices, faces=faces, process=False)
            if (len(faces) and np.isfinite(vertices).all() and
                    (not preserve_closed or candidate.is_watertight) and
                    (not mesh.is_winding_consistent or candidate.is_winding_consistent) and
                    not (candidate.area_faces <= 1e-12).any()):
                reduced = candidate
                break
    return reduced, {
        "facesBefore": int(len(mesh.faces)),
        "facesAfter": int(len(reduced.faces)),
        "reduced": len(reduced.faces) < len(mesh.faces),
        "targetFaceCount": target_faces,
        "targetReached": target_faces is None or len(reduced.faces) <= target_faces,
        "reductionLimited": target_faces is not None and len(reduced.faces) > target_faces,
    }


def topology(mesh):
    return {'watertight': bool(mesh.is_watertight),
            'windingConsistent': bool(mesh.is_winding_consistent),
            'components': int(len(mesh.split(only_watertight=False))),
            'degenerateFaces': int((mesh.area_faces <= 1e-12).sum())}


def unwrap(mesh, resolution):
    import xatlas
    import trimesh

    atlas = xatlas.Atlas()
    atlas.add_mesh(np.asarray(mesh.vertices, dtype=np.float32),
                   np.asarray(mesh.faces, dtype=np.uint32))
    packing = xatlas.PackOptions()
    packing.resolution = resolution
    packing.padding = PADDING
    packing.bilinear = True
    atlas.generate(pack_options=packing)
    if atlas.atlas_count != 1:
        raise ValueError('UV packing produced multiple atlases; try a lower face target.')
    vmapping, indices, uvs = atlas[0]
    textured = trimesh.Trimesh(
        vertices=np.asarray(mesh.vertices)[vmapping],
        faces=np.asarray(indices, dtype=np.uint32),
        # Keep smooth normals across the duplicated UV island boundaries.
        vertex_normals=np.asarray(mesh.vertex_normals)[vmapping],
        process=False,
    )
    textured.visual = trimesh.visual.texture.TextureVisuals(uv=np.asarray(uvs, dtype=np.float32))
    return textured


def _rasterize(mesh, resolution: int, query_colors, *, progress=None) -> Image.Image:
    """Query normalized sRGB at covered texels, at most QUERY_BATCH_SIZE at once.

    Trimesh UVs have their origin at bottom-left; PIL images start at top-left.
    Strip rasterization bounds temporary arrays even for a full-atlas triangle.
    """
    uv = np.asarray(mesh.visual.uv, dtype=np.float32).copy()
    uv[:, 1] = 1 - uv[:, 1]
    uv *= resolution
    vertices = np.asarray(mesh.vertices, dtype=np.float32)
    positions = np.zeros((resolution, resolution, 3), dtype=np.float32)
    covered = np.zeros((resolution, resolution), dtype=bool)
    for face in np.asarray(mesh.faces, dtype=np.int64):
        points = uv[face]
        lo = np.maximum(np.floor(points.min(axis=0)).astype(int), 0)
        hi = np.minimum(np.ceil(points.max(axis=0)).astype(int), resolution - 1)
        if np.any(lo > hi):
            continue
        a, b, c = points
        denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if abs(denominator) < 1e-10:
            continue
        for row in range(lo[1], hi[1] + 1, 32):
            end = min(row + 32, hi[1] + 1)
            x, y = np.meshgrid(np.arange(lo[0], hi[0] + 1, dtype=np.float32) + .5,
                               np.arange(row, end, dtype=np.float32) + .5)
            w0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator
            w1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator
            w2 = 1 - w0 - w1
            mask = (w0 >= -1e-6) & (w1 >= -1e-6) & (w2 >= -1e-6)
            region = np.s_[row:end, lo[0]:hi[0] + 1]
            positions[region][mask] = (np.stack((w0, w1, w2), axis=-1) @ vertices[face])[mask]
            covered[region][mask] = True
    indices = np.flatnonzero(covered)
    if not len(indices):
        raise ValueError('UV atlas contains no covered texels.')
    pixels = np.zeros((resolution, resolution, 3), dtype=np.uint8)
    for start in range(0, len(indices), QUERY_BATCH_SIZE):
        batch = indices[start:start + QUERY_BATCH_SIZE]
        colors = np.asarray(query_colors(positions.reshape(-1, 3)[batch]))
        if colors.shape != (len(batch), 3) or not np.isfinite(colors).all():
            raise ValueError('The engine returned invalid texture colors.')
        pixels.reshape(-1, 3)[batch] = np.rint(np.clip(colors, 0, 1) * 255).astype(np.uint8)
        if progress is not None:
            progress(min(start + len(batch), len(indices)) / len(indices))
    # Synchronous dilation: never wrap across atlas edges or overwrite covered
    # texels. Each iteration extends exactly one pixel into island gutters.
    for _ in range(PADDING):
        previous = covered.copy()
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)):
            dst = np.s_[max(0, dy):min(resolution, resolution + dy), max(0, dx):min(resolution, resolution + dx)]
            src = np.s_[max(0, -dy):min(resolution, resolution - dy), max(0, -dx):min(resolution, resolution - dx)]
            copy = ~covered[dst] & previous[src]
            pixels[dst][copy] = pixels[src][copy]
            covered[dst][copy] = True
    return Image.fromarray(pixels)


def bake(mesh, resolution: int, target_faces: int | None, query_colors, *, progress=None):
    if resolution not in (1024, 2048):
        raise ValueError("Baked textures support 1K or 2K resolution.")
    simplified, metrics = reduce_mesh(mesh, target_faces)
    quality = topology(simplified)  # UV seams duplicate vertices, not geometric holes.
    textured = unwrap(simplified, resolution)
    image = _rasterize(textured, resolution, query_colors, progress=progress)
    return textured, image, {**metrics, "textureResolution": f"{resolution // 1024}K",
                             'meshQuality': quality, 'colorSource': 'cached-scene',
                             'textureQueryBatchSize': QUERY_BATCH_SIZE}
