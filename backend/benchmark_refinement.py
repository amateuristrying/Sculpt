"""Compare extraction batches on the same cached scene, sequentially and offline.

Reports throughput and numerical parity, not semantic reconstruction quality.
Each run is an isolated worker. Alternating batch order reduces warm-cache bias.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys

from benchmark import validate_glb
from sculpt_backend.files import atomic_write_bytes
from sculpt_backend.scene_cache import CACHE_FILENAME, read_scene_cache
from sculpt_backend.refinement import refinement_settings


def mesh_parity(reference, candidate):
    import numpy as np
    import trimesh

    def geometry(path):
        scene = trimesh.load(path, force='scene', process=False)
        return list(scene.geometry.values())

    left, right = geometry(reference), geometry(candidate)
    same_topology = len(left) == len(right) and all(
        a.vertices.shape == b.vertices.shape and np.array_equal(a.faces, b.faces)
        for a, b in zip(left, right))
    max_error = max(float(np.abs(a.vertices - b.vertices).max()) for a, b in zip(left, right)) if same_topology else None
    # Loaded Color_0 may be represented as vertex attributes by trimesh.
    same_bytes = reference.read_bytes() == candidate.read_bytes()
    return {'identicalGlb': same_bytes, 'sameTopology': same_topology,
            'maxVertexDifference': max_error,
            'geometryEquivalent': same_topology and max_error <= 1e-6}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent-job', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--resolution', type=int, choices=[128, 192, 256], default=256)
    parser.add_argument('--repeats', type=int, choices=range(1, 6), default=3)
    parser.add_argument('--density', type=float, default=25)
    parser.add_argument('--smoothing', type=int, choices=range(11), default=0)
    parser.add_argument('--remove-fragments', action='store_true')
    parser.add_argument('--chunks', type=int, choices=[4096, 8192, 16384], nargs='+', default=[4096, 8192, 16384])
    args = parser.parse_args()
    if len(set(args.chunks)) != len(args.chunks):
        parser.error('Use distinct chunk sizes')
    settings = refinement_settings({'resolution': args.resolution, 'densityThreshold': args.density,
                                    'removeSmallComponents': args.remove_fragments, 'smoothingIterations': args.smoothing})
    parent = args.parent_job.resolve()
    metadata = json.loads((parent / 'metrics.json').read_text())
    cache = parent / CACHE_FILENAME
    read_scene_cache(cache, metadata['sourceSha256'])
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {'resolution': args.resolution, 'settings': settings, 'repeats': args.repeats, 'device': 'mps',
              'modelRevision': metadata['modelRevision'], 'runs': [],
              'note': 'RSS is process memory, not total GPU memory. Geometry parity is not a likeness score.'}
    reference = None
    for repeat in range(args.repeats):
        order = args.chunks[repeat % len(args.chunks):] + args.chunks[:repeat % len(args.chunks)]
        for chunk in order:
            directory = output / f'round-{repeat + 1}-chunk-{chunk}'
            directory.mkdir()
            request = {'operation': 'refine', 'sceneCachePath': str(cache),
                       'sourceSha256': metadata['sourceSha256'], 'outputPath': str(directory / 'mesh.glb'),
                       'device': 'mps', 'quality': 'balanced', 'queryChunkSize': chunk,
                       'refinement': settings}
            request_path = directory / 'request.json'
            request_path.write_text(json.dumps(request))
            print(f'Round {repeat + 1}: {args.resolution} grid, batch {chunk}', flush=True)
            worker = subprocess.run([sys.executable, str(Path(__file__).parent / 'worker.py'), '--request', str(request_path)],
                                    capture_output=True, text=True, timeout=180, env={**os.environ, 'OMP_NUM_THREADS': '4'})
            (directory / 'worker.log').write_text(worker.stderr)
            events = [json.loads(line) for line in worker.stdout.splitlines() if line.strip()]
            final = events[-1] if events else {}
            if worker.returncode or final.get('type') != 'result':
                raise RuntimeError(final.get('message', 'Benchmark worker failed; inspect worker.log'))
            artifact = directory / 'mesh.glb'
            validate_glb(artifact)
            if reference is None:
                reference = artifact
            parity = mesh_parity(reference, artifact)
            metrics = final['metrics']
            row = {'round': repeat + 1, 'chunkSize': chunk, **parity,
                   **{key: metrics[key] for key in ('surfaceSeconds', 'loadSeconds', 'totalSeconds', 'peakProcessMemoryMb', 'faces', 'vertices')}}
            report['runs'].append(row)
            atomic_write_bytes(output / 'report.json', json.dumps(report, indent=2).encode())
            print(f"  {row['surfaceSeconds']} s surface, {row['peakProcessMemoryMb']} MB RSS, parity={parity['geometryEquivalent']}", flush=True)
    report['summary'] = [{'chunkSize': chunk,
                          'medianSurfaceSeconds': statistics.median(row['surfaceSeconds'] for row in report['runs'] if row['chunkSize'] == chunk),
                          'maxProcessMemoryMb': max(row['peakProcessMemoryMb'] for row in report['runs'] if row['chunkSize'] == chunk)} for chunk in args.chunks]
    atomic_write_bytes(output / 'report.json', json.dumps(report, indent=2).encode())
    print(json.dumps(report['summary'], indent=2), flush=True)
    return 0 if all(row['geometryEquivalent'] for row in report['runs']) else 1


if __name__ == '__main__':
    raise SystemExit(main())
