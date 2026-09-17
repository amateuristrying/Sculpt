"""Small deterministic CPU triangle rasterizer. No OpenGL or new dependencies.

Coordinates are exported GLB (Y up). Azimuth follows TripoSR's Z-up cameras,
then applies the same proper rotation as Sculpt's exporter. Never auto-center,
resize or mirror a candidate mesh: those operations would hide regressions.
"""
import numpy as np
from PIL import Image

DEFAULT_CAMERA = {'azimuth': 0.0, 'elevation': 0.0, 'distance': 1.9, 'fov': 40.0}


def load_mesh(path):
    import trimesh
    from benchmark import validate_glb
    validate_glb(path)
    scene = trimesh.load(path, force='scene', process=False)
    vertices, faces, colors = [], [], []
    offset = 0
    for node in scene.graph.nodes_geometry:
        transform, name = scene.graph[node]
        mesh = scene.geometry[name]
        vertices.append(trimesh.transform_points(mesh.vertices, transform))
        faces.append(np.asarray(mesh.faces) + offset)
        attributes = getattr(mesh.visual, 'vertex_attributes', {})
        rgba = attributes.get('color')
        if rgba is None:
            rgba = getattr(mesh.visual, 'vertex_colors', np.full((len(mesh.vertices), 4), 180))
        colors.append(np.asarray(rgba)[:, :3].astype(float) / 255)
        offset += len(mesh.vertices)
    return np.concatenate(vertices), np.concatenate(faces), np.concatenate(colors)


def project(vertices, camera, size):
    azimuth, elevation = np.radians([camera['azimuth'], camera['elevation']])
    eye = camera['distance'] * np.array([np.cos(elevation) * np.cos(azimuth),
                                         np.sin(elevation), -np.cos(elevation) * np.sin(azimuth)])
    forward = -eye / np.linalg.norm(eye)
    right = np.cross(forward, [0, 1, 0])
    right /= np.linalg.norm(right)
    up = np.cross(right, forward)
    relative = vertices - eye
    depth = np.einsum('ij,j->i', relative, forward)
    if np.any(depth <= 0.01):
        raise ValueError('Mesh crosses the fixed camera near plane; choose a valid reference camera')
    focal = size / (2 * np.tan(np.radians(camera['fov']) / 2))
    xy = np.column_stack((size / 2 + focal * np.einsum('ij,j->i', relative, right) / depth,
                          size / 2 - focal * np.einsum('ij,j->i', relative, up) / depth))
    return xy, depth, eye


def render(mesh, camera=None, size=256):
    vertices, faces, colors = mesh
    xy, depths, eye = project(vertices, camera or DEFAULT_CAMERA, size)
    buffer = np.full((size, size), np.inf)
    linear = np.zeros((size, size, 3))
    triangles = xy[faces]
    lower = np.maximum(np.floor(triangles.min(axis=1)).astype(int), 0)
    upper = np.minimum(np.ceil(triangles.max(axis=1)).astype(int), size - 1)
    normals = np.cross(vertices[faces[:, 1]] - vertices[faces[:, 0]],
                       vertices[faces[:, 2]] - vertices[faces[:, 0]])
    normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-12)
    light = eye / np.linalg.norm(eye)
    # Two-sided soft lighting makes holes and disconnected surfaces visible.
    shade = 0.45 + 0.55 * np.abs(np.einsum('ij,j->i', normals, light))
    for index, (triangle, lo, hi) in enumerate(zip(triangles, lower, upper)):
        if np.any(lo > hi):
            continue
        a, b, c = triangle
        denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if abs(denominator) < 1e-10:
            continue
        x, y = np.meshgrid(np.arange(lo[0], hi[0] + 1) + .5, np.arange(lo[1], hi[1] + 1) + .5)
        w0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator
        w1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator
        w2 = 1 - w0 - w1
        weights = np.stack((w0, w1, w2), axis=-1) / depths[faces[index]]
        inv_depth = weights.sum(axis=-1)
        depth = 1 / np.maximum(inv_depth, 1e-12)
        region = np.s_[lo[1]:hi[1] + 1, lo[0]:hi[0] + 1]
        covered = (w0 >= -1e-9) & (w1 >= -1e-9) & (w2 >= -1e-9) & (depth < buffer[region])
        buffer[region][covered] = depth[covered]
        rgb = (weights @ colors[faces[index]]) / np.maximum(inv_depth[..., None], 1e-12)
        linear[region][covered] = (rgb * shade[index])[covered]
    mask = np.isfinite(buffer)
    srgb = np.where(linear <= .0031308, linear * 12.92, 1.055 * np.maximum(linear, 0) ** (1 / 2.4) - .055)
    pixels = np.rint(np.clip(srgb, 0, 1) * 255).astype(np.uint8)
    pixels[~mask] = [27, 30, 35]
    return Image.fromarray(pixels), mask


def silhouette(mesh, camera, size=128):
    """Fast polygon union for camera calibration only; scored renders use pixel centers."""
    from PIL import ImageDraw
    xy, _, _ = project(mesh[0], camera, size)
    image = Image.new('1', (size, size))
    draw = ImageDraw.Draw(image)
    for face in mesh[1]:
        draw.polygon([tuple(point) for point in xy[face]], fill=1)
    return np.asarray(image, dtype=bool)


def iou(reference, candidate):
    reference, candidate = np.asarray(reference, dtype=bool), np.asarray(candidate, dtype=bool)
    if reference.shape != candidate.shape:
        raise ValueError('Masks must use the same dimensions')
    union = np.count_nonzero(reference | candidate)
    return float(np.count_nonzero(reference & candidate) / union) if union else None


def mask_overlay(reference, candidate):
    pixels = np.full((*reference.shape, 3), [27, 30, 35], dtype=np.uint8)
    pixels[reference & candidate] = [225, 229, 236]
    pixels[reference & ~candidate] = [78, 190, 247]
    pixels[candidate & ~reference] = [246, 142, 84]
    return Image.fromarray(pixels)
