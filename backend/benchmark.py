"""Repeatable, sequential image reconstruction checks. No automatic image uploads.

Valid GLB/topology metrics are engineering checks, not perceptual quality scores.
Review each result visually before rating likeness or usefulness.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import struct
import time

from sculpt_backend.config import runtime_root


def validate_glb(file):
    """Check the actual exported artifact, independently of worker metrics."""
    import numpy as np
    import trimesh
    data = file.read_bytes()
    if len(data) < 20 or data[:4] != b'glTF' or struct.unpack_from('<II', data, 4) != (2, len(data)):
        raise ValueError('Invalid GLB header')
    size, kind = struct.unpack_from('<II', data, 12)
    if kind != 0x4E4F534A or size > len(data) - 20:
        raise ValueError('Invalid GLB JSON chunk')
    document = json.loads(data[20:20 + size])
    if any('uri' in item for key in ('buffers', 'images') for item in document.get(key, [])):
        raise ValueError('Benchmark GLB must be self-contained')
    scene = trimesh.load(file, force='scene', process=False)
    meshes = list(scene.geometry.values())
    if not meshes or any(not len(m.faces) or not np.isfinite(m.vertices).all() for m in meshes):
        raise ValueError('Empty or non-finite mesh')
    return {'faces': sum(len(m.faces) for m in meshes), 'vertices': sum(len(m.vertices) for m in meshes)}


def run_case(case, manifest_dir, output, quality):
    source = (manifest_dir / case['source']).resolve()
    folder = output / case['id']
    folder.mkdir(parents=True, exist_ok=True)
    request = folder / 'request.json'
    request.write_text(json.dumps({'sourcePath': str(source), 'outputPath': str(folder / 'mesh.glb'),
                                   'quality': quality, 'device': 'mps', 'background': case.get('background', 'auto')}))
    started = time.monotonic()
    result = subprocess.run([str(runtime_root() / 'venv/bin/python'), str(Path(__file__).parent / 'worker.py'), '--request', str(request)],
                            capture_output=True, text=True, timeout=1200, env={**os.environ, 'OMP_NUM_THREADS': '4'})
    (folder / 'worker.log').write_text(result.stderr)
    events = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    final = next((event for event in reversed(events) if event['type'] in ('result', 'error')), {})
    succeeded = result.returncode == 0 and final.get('type') == 'result' and (folder / 'mesh.glb').is_file()
    validation = validate_glb(folder / 'mesh.glb') if succeeded else None
    return {'id': case['id'], 'category': case.get('category', 'object'), 'quality': quality,
            'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'sourceCredit': case.get('credit'),
            'validResult': succeeded, 'expectationMet': succeeded == (case.get('expected', 'mesh') == 'mesh'),
            'validatedGeometry': validation,
            'wallSeconds': round(time.monotonic() - started, 2), 'metrics': final.get('metrics'),
            'error': final.get('message'), 'visualReview': 'pending'}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--quality', choices=['draft', 'balanced', 'high'], default='balanced')
    args = parser.parse_args()
    cases = json.loads(args.manifest.read_text())['cases']
    ids = [case['id'] for case in cases]
    if len(set(ids)) != len(ids) or any(not name or not all(c.isalnum() or c in '-_' for c in name) for name in ids):
        raise ValueError('Benchmark case IDs must be unique simple names')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if (output / 'report.json').exists():
        raise ValueError('Use a new output directory to preserve previous benchmark results')
    results = []
    for case in cases:
        print(f"Reconstructing {case['id']}…", flush=True)
        try:
            result = run_case(case, args.manifest.resolve().parent, output, args.quality)
        except Exception as error:
            result = {'id': case['id'], 'validResult': False, 'expectationMet': False, 'error': str(error)}
        results.append(result)
        (output / 'report.json').write_text(json.dumps({'engine': 'triposr', 'results': results}, indent=2))
        print(f"  {'PASS' if result['expectationMet'] else 'FAIL'}: {result.get('wallSeconds', 0)} s", flush=True)
    return 0 if all(result['expectationMet'] for result in results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
