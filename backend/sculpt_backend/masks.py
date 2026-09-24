"""Segmentation without loading the reconstruction model or spending a generation."""
import hashlib
from pathlib import Path
import resource
import sys
import time

from .images import load_source, segment_image


def prepare_mask(request, emit):
    started = time.monotonic()
    source = Path(request['sourcePath'])
    if not source.is_file() or source.stat().st_size > 30 * 1024 * 1024:
        raise ValueError('Source image is missing or exceeds 30 MB.')
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    if request.get('sourceSha256') != digest:
        raise ValueError('The source image changed after import. Import it again.')
    output = Path(request['outputPath'])
    output.mkdir(parents=True, exist_ok=False)
    emit('analyzing', 5, 'Opening the source image')
    image = load_source(source)
    # RGB behind transparent pixels stays available for additive brush corrections.
    image.convert('RGB').save(output / 'preview.png')
    emit('analyzing', 20, 'Finding the foreground · local CPU')
    masked = segment_image(image, request.get('background', 'auto'))
    masked.getchannel('A').save(output / 'mask.png')
    emit('preparing', 95, 'Foreground ready to review')
    return {'sourceSha256': digest, 'width': image.width, 'height': image.height,
            'totalSeconds': round(time.monotonic() - started, 3),
            'peakProcessMemoryMb': round(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss /
                (1024 * 1024 if sys.platform == 'darwin' else 1024), 1)}
