import hashlib
import json
from pathlib import Path
import re
import time
import urllib.error
import urllib.request

DEFAULT_MANIFEST = Path(__file__).resolve().parents[1] / 'evaluation/photos.json'


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_manifest(path):
    data = json.loads(path.read_text())
    if data.get('schemaVersion') != 1 or not data.get('cases'):
        raise ValueError('Expected a version 1 evaluation manifest with cases')
    ids = set()
    hashes = set()
    for case in data['cases']:
        if not re.fullmatch(r'[a-z0-9][a-z0-9_-]{0,79}', case['id']) or case['id'] in ids:
            raise ValueError('Case IDs must be unique simple names')
        ids.add(case['id'])
        if not re.fullmatch('[a-f0-9]{64}', case['sha256']) or case['sha256'] in hashes:
            raise ValueError('Photos must have unique SHA-256 hashes')
        hashes.add(case['sha256'])
        if case['license'] not in {'CC0-1.0', 'Public-Domain', 'Self-shot'}:
            raise ValueError('Evaluation photos must be CC0, public domain, or self-shot')
        if not case.get('sourceUrl') or not case.get('licenseEvidence'):
            raise ValueError('Each photo needs source and license evidence')
    return data


def verify_photo(case, manifest_path):
    path = (manifest_path.parent / case['source']).resolve()
    if not path.is_file() or sha256(path) != case['sha256']:
        raise ValueError(f"{case['id']}: missing or changed photo; run evaluation fetch")
    return path


def fetch_photos(manifest_path):
    """Sequential, hash-pinned downloads; never silently accept replaced source bytes."""
    from PIL import Image
    for case in load_manifest(manifest_path)['cases']:
        destination = (manifest_path.parent / case['source']).resolve()
        if destination.exists():
            verify_photo(case, manifest_path)
            print(f"Verified {case['id']}", flush=True)
            continue
        url = case['downloadUrl']
        if not url.startswith('https://'):
            raise ValueError('Photo download URLs must use HTTPS')
        destination.parent.mkdir(parents=True, exist_ok=True)
        request = urllib.request.Request(url, headers={'User-Agent': 'SculptEvaluation/0.1 (github.com/amateuristrying/Sculpt)'})
        for attempt in range(5):
            try:
                with urllib.request.urlopen(request, timeout=90) as response:
                    payload = response.read(30 * 1024 * 1024 + 1)
                break
            except urllib.error.HTTPError as error:
                if error.code != 429 or attempt == 4:
                    raise
                retry = error.headers.get('Retry-After', '60')
                delay = max(10, int(retry)) if retry.isdigit() else 60
                if delay > 600:
                    raise ValueError('Photo host asks for a long delay; retry fetching later') from error
                print(f'Photo host rate limit; retry in {delay} seconds', flush=True)
                time.sleep(delay)
        if hashlib.sha256(payload).hexdigest() != case['sha256']:
            raise ValueError(f"{case['id']}: download differs from pinned photo; review source manually")
        temporary = destination.with_suffix(destination.suffix + '.part')
        try:
            temporary.write_bytes(payload)
            with Image.open(temporary) as image:
                if image.width * image.height > 40_000_000:
                    raise ValueError('Photo exceeds worker pixel limit')
                image.verify()
            temporary.replace(destination)
        finally:
            temporary.unlink(missing_ok=True)
        print(f"Downloaded {case['id']}", flush=True)
        time.sleep(2)
