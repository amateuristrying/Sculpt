"""Isolated CPU mask evaluation on the pinned CC0 photo set.

The parity check compares normalized TripoSR inputs against the saved baseline.
It is a regression check, not an independent segmentation accuracy score.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from sculpt_eval.dataset import DEFAULT_MANIFEST, load_manifest, verify_photo
from sculpt_eval.runner import machine_info, write_json


def run_one(source, output):
    from sculpt_backend.config import configure_environment, runtime_root
    from sculpt_backend.masks import prepare_mask
    configure_environment(runtime_root())
    return prepare_mask({'sourcePath': str(source), 'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                         'outputPath': str(output)}, lambda *args: None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--source', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.source:
        print(json.dumps(run_one(args.source, args.output)))
        return
    dataset = load_manifest(args.manifest)
    for case in dataset['cases']: verify_photo(case, args.manifest)
    hardware = machine_info()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'schemaVersion': 1, 'hardware': hardware, 'datasetSha256': hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
              'model': 'u2net', 'provider': 'CPUExecutionProvider', 'results': []}
    for case in dataset['cases']:
        started = time.monotonic()
        folder = args.output / case['id']
        source = (args.manifest.parent / case['source']).resolve()
        process = subprocess.run([sys.executable, __file__, '--source', str(source), '--output', str(folder)],
            capture_output=True, text=True, timeout=180, env={**os.environ, 'OMP_NUM_THREADS': '4'})
        row = {'id': case['id'], 'sourceSha256': case['sha256'], 'success': process.returncode == 0,
               'wallSeconds': round(time.monotonic() - started, 3)}
        if process.returncode == 0:
            row.update(json.loads(process.stdout))
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
            row['maskSha256'] = hashlib.sha256((folder / 'mask.png').read_bytes()).hexdigest()
        else: row['error'] = process.stderr[-1500:]
        report['results'].append(row); write_json(args.output / 'report.json', report)
        print(f"{case['id']}: {'OK' if row['success'] else 'FAILED'} {row['wallSeconds']} s", flush=True)


if __name__ == '__main__': main()
