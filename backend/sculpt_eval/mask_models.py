"""Evaluation-only mask adapters. These do not alter Sculpt's installed runtime."""
import hashlib
from pathlib import Path
import resource
import sys
import time

BIREFNET_BYTES = 972666916
BIREFNET_MD5 = '7a35a0141cbbc80de11d9c9a28f52697'
# Filled from the completed, upstream-MD5-verified rembg release artifact.
BIREFNET_SHA256 = '58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c'


def verify_birefnet(path):
    path = Path(path)
    if path.stat().st_size != BIREFNET_BYTES:
        raise ValueError('BiRefNet download is incomplete; expected 972,666,916 bytes.')
    digest, legacy = hashlib.sha256(), hashlib.md5()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block); legacy.update(block)
    if legacy.hexdigest() != BIREFNET_MD5 or digest.hexdigest() != BIREFNET_SHA256:
        raise ValueError('BiRefNet model checksum does not match the reviewed release.')
    return digest.hexdigest()


def birefnet_mask(source, output, weights):
    started = time.monotonic()
    from sculpt_backend.images import load_source
    import numpy as np
    image = load_source(source)
    output.mkdir(parents=True, exist_ok=False)
    image.convert('RGB').save(output / 'preview.png')
    alpha = image.getchannel('A')
    existing = int(np.asarray(alpha).min()) < 250
    if not existing:
        import onnxruntime as ort
        from rembg.sessions.birefnet_general import BiRefNetSessionGeneral

        class OfflineBiRefNet(BiRefNetSessionGeneral):
            @classmethod
            def download_models(cls, *args, **kwargs):
                # Parent verifies the pinned artifact before admitting any case.
                # Never call rembg's download-on-missing implementation.
                return str(weights)

        options = ort.SessionOptions()
        options.intra_op_num_threads = options.inter_op_num_threads = 4
        session = OfflineBiRefNet('birefnet-general', options, providers=['CPUExecutionProvider'])
        alpha = session.predict(image)[0]
    alpha.save(output / 'mask.png')
    return {'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
            'width': image.width, 'height': image.height, 'maskOrigin': 'existing-alpha' if existing else 'birefnet-general',
            'totalSeconds': round(time.monotonic() - started, 3),
            'peakProcessMemoryMb': round(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss /
                (1024 * 1024 if sys.platform == 'darwin' else 1024), 1)}
