"""Isolated CPU mask evaluation on the pinned CC0 photo set.

The parity check compares normalized TripoSR inputs against the saved baseline.
It is a regression check, not an independent segmentation accuracy score.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from sculpt_eval.dataset import DEFAULT_MANIFEST, load_manifest, verify_photo
from sculpt_eval.runner import machine_info, write_json


def run_one(source, output, model, weights):
    from sculpt_backend.config import configure_environment, runtime_root
    from sculpt_backend.masks import prepare_mask
    configure_environment(runtime_root())
    if model == 'birefnet-general':
        from sculpt_eval.mask_models import birefnet_mask
        return birefnet_mask(source, output, weights)
    return prepare_mask({'sourcePath': str(source), 'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                         'outputPath': str(output)}, lambda *args: None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--model', choices=['u2net', 'birefnet-general'], default='u2net')
    parser.add_argument('--weights', type=Path, help='Explicit evaluation-only BiRefNet ONNX artifact; never downloaded implicitly')
    parser.add_argument('--reference-masks', type=Path, help='Report mask agreement IoU; automatic masks are not ground truth')
    parser.add_argument('--case', action='append', dest='case_ids', help='Run only this case; repeat to select several')
    parser.add_argument('--source', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.source:
        print(json.dumps(run_one(args.source, args.output, args.model, args.weights)))
        return
    dataset = load_manifest(args.manifest)
    for case in dataset['cases']: verify_photo(case, args.manifest)
    from sculpt_backend.config import SPEC, runtime_root
    weights_sha = None
    if args.model == 'birefnet-general':
        if args.weights is None: raise ValueError('--weights is required for the optional BiRefNet evaluation')
        from sculpt_eval.mask_models import verify_birefnet
        weights_sha = verify_birefnet(args.weights)
    else:
        weights = runtime_root() / 'models/background/u2net.onnx'
        with weights.open('rb') as stream:
            weights_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
        if weights_sha != SPEC['artifacts']['models/background/u2net.onnx']:
            raise ValueError('U2Net weights differ from the pinned runtime; repair the runtime first')
    cases = dataset['cases']
    if args.case_ids:
        if not set(args.case_ids) <= {case['id'] for case in cases}: raise ValueError('Unknown evaluation case')
        cases = [case for case in cases if case['id'] in args.case_ids]
    hardware = machine_info()
    git = subprocess.run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True)
    dirty = subprocess.run(['git', 'status', '--porcelain'], capture_output=True, text=True)
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'schemaVersion': 1, 'hardware': hardware, 'datasetSha256': hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
              'createdAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'codeCommit': git.stdout.strip(), 'codeDirty': bool(dirty.stdout.strip()),
              'model': args.model, 'modelSha256': weights_sha, 'memoryFloorGb': 1.0,
              'threads': 4, 'timeoutSeconds': 300,
              'referenceMasks': str(args.reference_masks) if args.reference_masks else None,
              'provider': 'CPUExecutionProvider', 'results': []}
    for case in cases:
        started = time.monotonic()
        folder = args.output / case['id']
        source = (args.manifest.parent / case['source']).resolve()
        command = [sys.executable, __file__, '--source', str(source), '--output', str(folder), '--model', args.model]
        if args.weights: command += ['--weights', str(args.weights.resolve())]
        import psutil
        minimum_available = psutil.virtual_memory().available / 1024**3
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            env={**os.environ, 'OMP_NUM_THREADS': '4'})
        stopped = None
        peak_observed_rss = 0
        observed = psutil.Process(process.pid)
        try:
            while True:
                try: peak_observed_rss = max(peak_observed_rss, observed.memory_info().rss)
                except psutil.NoSuchProcess: pass
                minimum_available = min(minimum_available, psutil.virtual_memory().available / 1024**3)
                if minimum_available < 1.0 or time.monotonic() - started > 300:
                    stopped = 'Stopped at the 1 GB available-memory floor' if minimum_available < 1.0 else 'Mask worker exceeded five minutes'
                    process.terminate()
                    try: stdout, stderr = process.communicate(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill(); stdout, stderr = process.communicate()
                    break
                try:
                    stdout, stderr = process.communicate(timeout=0.2)
                    break
                except subprocess.TimeoutExpired: pass
        finally:
            if process.poll() is None:
                process.kill(); process.communicate()
        row = {'id': case['id'], 'sourceSha256': case['sha256'], 'success': process.returncode == 0 and stopped is None,
               'minimumAvailableGbObserved': round(minimum_available, 3),
               'peakObservedProcessMemoryMb': round(peak_observed_rss / 1024**2, 1),
               'wallSeconds': round(time.monotonic() - started, 3)}
        if row['success']:
            try:
                row.update(json.loads(stdout))
                from sculpt_backend.images import prepare_image
                prepare_image(source, folder / 'input.png', mask_path=folder / 'mask.png')
                if args.baseline:
                    old = args.baseline / case['id'] / 'input.png'
                    from PIL import Image
                    import numpy as np
                    before = np.asarray(Image.open(old)).astype(int)
                    after = np.asarray(Image.open(folder / 'input.png')).astype(int)
                    row['inputMaxPixelDelta'] = int(np.max(np.abs(after - before)))
                    row['inputByteIdentical'] = old.read_bytes() == (folder / 'input.png').read_bytes()
                if args.reference_masks:
                    from PIL import Image
                    import numpy as np
                    from sculpt_eval.render import iou
                    reference = Image.open(args.reference_masks / case['id'] / 'mask.png')
                    actual = Image.open(folder / 'mask.png')
                    row['referenceMaskAgreementIou'] = iou(np.asarray(reference) > 32, np.asarray(actual) > 32)
                row['maskSha256'] = hashlib.sha256((folder / 'mask.png').read_bytes()).hexdigest()
            except Exception as error:
                row['success'] = False
                row['error'] = str(error)
        else: row['error'] = stopped or stderr[-1500:]
        report['results'].append(row); write_json(args.output / 'report.json', report)
        print(f"{case['id']}: {'OK' if row['success'] else 'FAILED'} {row['wallSeconds']} s", flush=True)
    return 0 if all(row['success'] for row in report['results']) else 1


if __name__ == '__main__': raise SystemExit(main())
