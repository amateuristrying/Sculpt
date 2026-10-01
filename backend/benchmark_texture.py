"""Bake cached eval meshes, then write the existing four-view comparison report.

One isolated process per case; no image inference, geometry re-extraction, or
camera refitting. Source GLB and scene hashes are retained for reproducibility.
"""
import argparse
import copy
import datetime
import hashlib
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import time


def bake_case(parent, output, device, resolution, target):
    import numpy as np
    import torch
    import trimesh
    from sculpt_backend.config import configure_environment, runtime_root
    from sculpt_backend.scene_cache import read_scene_cache
    from sculpt_backend.texture import bake
    from sculpt_backend.triposr import load_model
    from sculpt_backend.export import export_textured_glb
    from sculpt_eval.runner import write_json
    from benchmark import validate_glb

    started = time.monotonic()
    root = runtime_root()
    configure_environment(root)
    if device == 'mps' and not torch.backends.mps.is_available():
        raise ValueError('Metal is not available; select CPU explicitly instead.')
    torch.set_num_threads(4)
    if device == 'mps':
        torch.mps.set_per_process_memory_fraction(.65)
    metadata = json.loads((parent / 'metrics.json').read_text())
    codes, _ = read_scene_cache(parent / 'scene-cache.npz', metadata['sourceSha256'])
    scene = torch.from_numpy(codes).to(device)
    model = load_model(root, device, decoder_only=True)
    model.renderer.set_chunk_size(8192)
    mesh_scene = trimesh.load(parent / 'mesh.glb', force='scene', process=False)
    mesh = mesh_scene.to_geometry()
    rotation = np.array([[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [0, 0, 0, 1]], float)
    mesh.apply_transform(rotation.T)  # Saved Y-up mesh back to the neural field's Z-up.
    def query(points):
        with torch.inference_mode():
            tensor = torch.as_tensor(points, dtype=scene.dtype, device=device)
            return model.renderer.query_triplane(model.decoder, tensor, scene[0])['color'].cpu().numpy()
    load_seconds = time.monotonic() - started
    bake_started = time.monotonic()
    textured, image, metrics = bake(mesh, resolution, target, query)
    metrics['bakeSeconds'] = time.monotonic() - bake_started
    textured.apply_transform(rotation)
    output.mkdir(parents=True, exist_ok=False)
    if (parent / 'request.json').is_file():
        (output / 'request.json').write_bytes((parent / 'request.json').read_bytes())
    export_textured_glb(textured, image, output / 'mesh.glb')
    validate_glb(output / 'mesh.glb')
    metrics.update(device=device, operation='bake-existing-mesh', faces=len(textured.faces),
                   vertices=len(textured.vertices), loadSeconds=load_seconds,
                   totalSeconds=time.monotonic() - started,
                   peakProcessMemoryMb=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024**2 if sys.platform == 'darwin' else 1024),
                   sourceSha256=metadata['sourceSha256'],
                   parentMeshSha256=hashlib.sha256((parent / 'mesh.glb').read_bytes()).hexdigest(),
                   sceneCacheSha256=hashlib.sha256((parent / 'scene-cache.npz').read_bytes()).hexdigest(),
                   fileBytes=(output / 'mesh.glb').stat().st_size)
    write_json(output / 'metrics.json', metrics)


def score_color_fidelity(baseline, candidate, device):
    """Surface-weighted color error against the cached field, not photo likeness.

    Each representation is scored on its own surface after decimation. A lower
    error does not imply more accurate geometry or correct unseen surfaces.
    """
    import numpy as np
    import torch
    from sculpt_backend.config import runtime_root, configure_environment
    from sculpt_backend.scene_cache import read_scene_cache
    from sculpt_backend.triposr import load_model
    from sculpt_eval.render import load_mesh, sample_texture
    from sculpt_eval.runner import write_json
    root = runtime_root()
    configure_environment(root)
    if device == 'mps' and not torch.backends.mps.is_available():
        raise ValueError('Metal is not available.')
    torch.set_num_threads(4)
    model = load_model(root, device, decoder_only=True)
    model.renderer.set_chunk_size(8192)
    results = []
    for row in json.loads((candidate / 'report.json').read_text())['results']:
        if not row['validResult']:
            continue
        case = row['id']
        codes, _ = read_scene_cache(baseline / case / 'scene-cache.npz', row['sourceSha256'])
        scene = torch.from_numpy(codes).to(device)
        scores = {}
        for label, folder in [('vertex', baseline), ('texture', candidate)]:
            mesh = load_mesh(folder / case / 'mesh.glb')
            vertices, faces, colors = mesh
            areas = np.linalg.norm(np.cross(vertices[faces[:, 1]] - vertices[faces[:, 0]],
                                             vertices[faces[:, 2]] - vertices[faces[:, 0]]), axis=1)
            rng = np.random.default_rng(420)
            chosen = rng.choice(len(faces), 8192, p=areas / areas.sum())
            uv = rng.random((8192, 2))
            uv[uv.sum(axis=1) > 1] = 1 - uv[uv.sum(axis=1) > 1]
            weights = np.column_stack((1 - uv.sum(axis=1), uv))
            points = (vertices[faces[chosen]] * weights[..., None]).sum(axis=1)
            if label == 'vertex':
                rgb = (colors[faces[chosen]] * weights[..., None]).sum(axis=1)
            else:
                coords = (mesh.uvs[faces[chosen]] * weights[..., None]).sum(axis=1)
                rgb = sample_texture(mesh.textures[0], coords)
            srgb = np.where(rgb <= .0031308, rgb * 12.92, 1.055 * np.maximum(rgb, 0) ** (1 / 2.4) - .055)
            with torch.inference_mode():
                tensor = torch.as_tensor(np.column_stack((points[:, 0], -points[:, 2], points[:, 1])), dtype=scene.dtype, device=device)
                predicted = model.renderer.query_triplane(model.decoder, tensor, scene[0])['color'].cpu().numpy()
            scores[label + 'MaeSrgb255'] = float(np.abs(srgb - predicted).mean() * 255)
        results.append({'id': case, **scores})
    report = {'method': 'area-weighted-own-surface-cached-field-srgb-mae-v1', 'samplesPerSurface': 8192,
              'device': device, 'note': 'Color storage fidelity, not photo similarity or 3D correctness. Different geometry is sampled independently with the same seed.',
              'results': results, 'summary': {key: float(np.mean([r[key] for r in results])) if results else None for key in ['vertexMaeSrgb255', 'textureMaeSrgb255']}}
    write_json(candidate / 'color-fidelity.json', report)
    print(json.dumps(report['summary']), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--device', choices=['mps', 'cpu'], required=True)
    parser.add_argument('--resolution', type=int, choices=[1024, 2048], default=1024)
    parser.add_argument('--target-faces', type=int, default=10000)
    parser.add_argument('--score-only', action='store_true', help='Score existing baked results without rebuilding')
    parser.add_argument('--case', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.score_only:
        score_color_fidelity(args.baseline, args.output, args.device)
        return 0
    if args.case:
        bake_case(args.baseline / args.case, args.output / args.case, args.device, args.resolution, args.target_faces)
        return 0
    from sculpt_eval.report import read_run, compare
    from sculpt_eval.runner import write_json, machine_info
    from sculpt_eval.dataset import sha256
    baseline = read_run(args.baseline)
    args.output.mkdir(parents=True, exist_ok=False)
    report = {**copy.deepcopy(baseline), 'device': args.device, 'hardware': machine_info(), 'results': [],
              'createdAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'operation': 'bake-existing-mesh', 'textureResolution': args.resolution, 'targetFaces': args.target_faces,
              'baselineReportSha256': sha256(args.baseline / 'report.json'),
              'codeCommit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
              'codeDirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], text=True).strip())}
    for row in baseline['results']:
        case = row['id']
        print(f'Baking {len(report["results"]) + 1}/{len(baseline["results"])}: {case}', flush=True)
        started = time.monotonic()
        result = {'id': case, 'sourceSha256': row['sourceSha256'], 'validResult': False}
        command = [sys.executable, __file__, '--baseline', str(args.baseline), '--output', str(args.output),
                   '--device', args.device, '--resolution', str(args.resolution), '--target-faces', str(args.target_faces), '--case', case]
        try:
            worker = subprocess.run(command, capture_output=True, text=True, timeout=300,
                                    env={**os.environ, 'OMP_NUM_THREADS': '4', 'PYTORCH_ENABLE_MPS_FALLBACK': '1'})
            (args.output / f'{case}.log').write_text(worker.stdout + worker.stderr)
            if worker.returncode:
                raise ValueError(worker.stderr[-1500:])
            result.update(validResult=True, metrics=json.loads((args.output / case / 'metrics.json').read_text()))
        except Exception as error:
            result['error'] = str(error)
        result['wallSeconds'] = round(time.monotonic() - started, 2)
        report['results'].append(result)
        write_json(args.output / 'report.json', report)
        print(f'  {"OK" if result["validResult"] else "FAILED"}: {result["wallSeconds"]} s', flush=True)
    score_color_fidelity(args.baseline, args.output, args.device)
    if args.reference:
        compare(args.baseline, args.output, args.reference, args.output / 'comparison')
    return 0 if all(row['validResult'] for row in report['results']) else 1


if __name__ == '__main__':
    raise SystemExit(main())
