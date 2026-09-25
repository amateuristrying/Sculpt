import datetime
import json
import platform
import subprocess

from benchmark import run_case
from sculpt_backend.config import SPEC
from sculpt_backend.files import atomic_write_bytes
from sculpt_backend.memory import check_memory
from .dataset import load_manifest, sha256, verify_photo


def write_json(path, value):
    atomic_write_bytes(path, (json.dumps(value, indent=2, allow_nan=False) + '\n').encode())


def machine_info():
    data = {'os': platform.system(), 'osVersion': platform.release(), 'architecture': platform.machine()}
    if platform.system() == 'Darwin':
        for key, name in [('machdep.cpu.brand_string', 'chip'), ('hw.memsize', 'ramBytes')]:
            result = subprocess.run(['sysctl', '-n', key], capture_output=True, text=True)
            if result.returncode == 0:
                data[name] = int(result.stdout) if name == 'ramBytes' else result.stdout.strip()
            else:
                data[name] = None
                data.setdefault('probeErrors', {})[key] = result.stderr.strip()[:500]
        if data.get('ramBytes') is None:
            import psutil
            data['ramBytes'] = psutil.virtual_memory().total
            data['ramSource'] = 'psutil'
    return data


def run(manifest_path, output, quality, masks=None, device='mps', case_ids=None):
    if device not in {'mps', 'cpu'}:
        raise ValueError('Evaluation device must be explicit: mps or cpu')
    manifest_path = manifest_path.resolve()
    dataset = load_manifest(manifest_path)
    cases = dataset['cases']
    if case_ids is not None:
        if not case_ids or not set(case_ids) <= {case['id'] for case in cases}:
            raise ValueError('Select one or more known evaluation case IDs')
        cases = [case for case in cases if case['id'] in case_ids]
    # Verify the entire set before spending GPU time. No partial source substitution.
    for case in dataset['cases']:
        verify_photo(case, manifest_path)
    # Avoid creating 34 identical failed jobs when the machine is already full.
    # The worker still checks again after loading Python/PyTorch for every job.
    import psutil
    check_memory(quality, psutil.virtual_memory().available / (1024 ** 3))
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    git = subprocess.run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True)
    dirty = subprocess.run(['git', 'status', '--porcelain'], capture_output=True, text=True)
    report = {'schemaVersion': 1, 'dataset': dataset, 'datasetSha256': sha256(manifest_path),
              'createdAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'engine': 'triposr', 'quality': quality, 'device': device, 'hardware': machine_info(),
              'selectedCaseIds': [case['id'] for case in cases],
              'codeCommit': git.stdout.strip(), 'codeDirty': bool(dirty.stdout.strip()),
              'runtimeSpec': SPEC, 'results': []}
    write_json(output / 'report.json', report)
    for case in cases:
        print(f"Reconstructing {case['id']} on {'Metal' if device == 'mps' else 'CPU'} ({quality})…", flush=True)
        try:
            result = run_case(case, manifest_path.parent, output, quality, masks=masks, device=device)
            # Source hashes in the run and manifest must agree, including on repeat runs.
            if result['sourceSha256'] != case['sha256']:
                raise ValueError('Source changed during reconstruction')
        except Exception as error:
            result = {'id': case['id'], 'sourceSha256': case['sha256'], 'category': case['category'],
                      'validResult': False, 'expectationMet': False, 'error': str(error)}
        report['results'].append(result)
        write_json(output / 'report.json', report)
        print(f"  {'MESH' if result['validResult'] else 'FAILED'}: {result.get('wallSeconds', 0)} s", flush=True)
    # Failures remain in the comparison, not silently dropped from the dataset.
    return 0 if all(row['validResult'] for row in report['results']) else 1
