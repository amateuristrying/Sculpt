"""Compare Rust-exported STLs with a saved evaluation run; no inference required."""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import trimesh


def verify(source: Path, exports: Path, height: float) -> dict:
    results = []
    for path in sorted(source.glob('*/mesh.glb')):
        destination = exports / f'{path.parent.name}.stl'
        original = trimesh.load(path, force='scene', process=False).to_geometry()
        exported = trimesh.load(destination, force='mesh', process=False)
        before_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        low, high = original.bounds
        scale = height / (high[1] - low[1])
        expected = original.copy()
        points = expected.vertices.copy()
        points -= [low[0] + (high[0]-low[0])/2, low[1], low[2] + (high[2]-low[2])/2]
        expected.vertices = points[:, [0, 2, 1]] * [scale, -scale, scale]
        # STL repeats corners; UV seams also duplicate GLB vertices. Weld exact
        # f32 positions for both, ignoring normals and UVs, without repairing faces.
        expected.vertices = expected.vertices.astype(np.float32)
        expected.merge_vertices(merge_tex=True, merge_norm=True, digits_vertex=8)
        exported.merge_vertices(merge_tex=True, merge_norm=True, digits_vertex=8)
        # Triangle ordering and vertex numbering may differ; oriented normals and
        # volume catch winding errors that unordered point comparisons would miss.
        def canonical(points):
            rounded = np.round(points, 4)
            return rounded[np.lexsort(rounded.T[::-1])]
        assert len(expected.faces) == len(exported.faces), path
        assert np.array_equal(canonical(expected.triangles.reshape(-1, 3)),
                              canonical(exported.triangles.reshape(-1, 3))), path
        assert expected.is_watertight == exported.is_watertight, path
        assert expected.is_winding_consistent == exported.is_winding_consistent, path
        assert expected.body_count == exported.body_count, path
        assert np.isclose(expected.volume, exported.volume, rtol=1e-5, atol=1e-5), path
        assert np.isclose(exported.extents[2], height, atol=1e-4), path
        assert abs(exported.bounds[0, 2]) <= 1e-5, path
        assert before_hash == hashlib.sha256(path.read_bytes()).hexdigest(), path
        results.append({
            'case': path.parent.name, 'sourceSha256': before_hash,
            'stlSha256': hashlib.sha256(destination.read_bytes()).hexdigest(),
            'facesBefore': len(original.faces), 'facesAfter': len(exported.faces),
            'closedBefore': bool(expected.is_watertight), 'closedAfter': bool(exported.is_watertight),
            'componentsBefore': int(expected.body_count), 'componentsAfter': int(exported.body_count),
            'heightMm': float(exported.extents[2]), 'bytes': destination.stat().st_size,
        })
    if not results:
        raise ValueError('No evaluation meshes found')
    return {'heightMm': height, 'cases': results, 'passed': len(results)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('exports', type=Path)
    parser.add_argument('--height-mm', type=float, default=100)
    args = parser.parse_args()
    report = verify(args.source, args.exports, args.height_mm)
    (args.exports / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"{report['passed']} exports passed: faces, positions, winding, volume, components, closed surfaces and height")
