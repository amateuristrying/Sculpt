"""Offline visual comparison of two mask runs. Agreement IoU is not accuracy."""
import argparse
import base64
import hashlib
import html
import io
import json
from pathlib import Path
import statistics

import numpy as np
from PIL import Image

from sculpt_eval.render import iou
from sculpt_eval.runner import write_json


def image_tag(image, label):
    image = image.copy(); image.thumbnail((420, 420))
    stream = io.BytesIO(); image.save(stream, format='PNG')
    return f'<figure><img alt="{html.escape(label, quote=True)}" src="data:image/png;base64,{base64.b64encode(stream.getvalue()).decode()}"><figcaption>{html.escape(label)}</figcaption></figure>'


def read_case(folder, row):
    if not row or not row.get('success'):
        return None, '<p class="failure">' + html.escape((row or {}).get('error', 'Missing result')) + '</p>'
    path = folder / row['id'] / 'mask.png'
    if hashlib.sha256(path.read_bytes()).hexdigest() != row['maskSha256']:
        raise ValueError(f"Mask changed since evaluation: {row['id']}")
    with Image.open(path) as opened:
        mask = opened.convert('L')
    with Image.open(folder / row['id'] / 'preview.png') as opened:
        photo = opened.convert('RGBA')
    if photo.size != mask.size:
        raise ValueError('Mask dimensions must match source preview coordinates')
    photo.putalpha(mask)
    return np.asarray(mask) > 32, image_tag(photo, 'Selected foreground') + image_tag(mask, 'Grayscale mask')


def compare(left, right, output):
    a, b = [json.loads((folder / 'report.json').read_text()) for folder in (left, right)]
    if a['datasetSha256'] != b['datasetSha256']:
        raise ValueError('Mask comparison requires the same source dataset')
    by_id = [{row['id']: row for row in report['results']} for report in (a, b)]
    keys = list(dict.fromkeys([*by_id[0], *by_id[1]]))
    sections, rows = [], []
    for key in keys:
        before, after = (mapping.get(key) for mapping in by_id)
        if before and after and before['sourceSha256'] != after['sourceSha256']:
            raise ValueError(f'Source identity mismatch: {key}')
        mask_a, pane_a = read_case(left, before)
        mask_b, pane_b = read_case(right, after)
        agreement = iou(mask_a, mask_b) if mask_a is not None and mask_b is not None else None
        row = {'id': key, 'sourceSha256': (before or after)['sourceSha256'], 'maskAgreementIou': agreement,
               'leftSuccess': bool(before and before.get('success')), 'rightSuccess': bool(after and after.get('success'))}
        rows.append(row)
        label = f'{agreement:.4f}' if agreement is not None else 'unavailable'
        metrics = []
        for report, item in [(a, before), (b, after)]:
            metric = f"{item['totalSeconds']:.2f} s · {item['peakProcessMemoryMb']:.0f} MB peak RSS" if item and item.get('success') else 'No usable result'
            metrics.append(f"{html.escape(report['model'])} — {metric}")
        source_folder = left if before and before.get('success') else right
        original = source_folder / key / 'preview.png'
        source_html = '<p>Source preview unavailable</p>'
        if original.is_file():
            with Image.open(original) as opened:
                source_html = image_tag(opened, 'Source photo')
        sections.append(f'<section><h2>{html.escape(key)}</h2><p>Mask agreement IoU: {label}</p><div class="row"><div>{source_html}</div><div><h3>{metrics[0]}</h3>{pane_a}</div><div><h3>{metrics[1]}</h3>{pane_b}</div></div></section>')
    stats = []
    for report in (a, b):
        good = [row for row in report['results'] if row.get('success')]
        stats.append({'model': report['model'], 'attempted': len(report['results']), 'successes': len(good),
                      'medianWorkerSeconds': statistics.median(row['totalSeconds'] for row in good) if good else None,
                      'medianWallSeconds': statistics.median(row['wallSeconds'] for row in good) if good and all('wallSeconds' in row for row in good) else None,
                      'maxProcessMemoryMb': max((row['peakProcessMemoryMb'] for row in good), default=None)})
    values = [row['maskAgreementIou'] for row in rows if row['maskAgreementIou'] is not None]
    result = {'schemaVersion': 1, 'datasetSha256': a['datasetSha256'], 'method': 'source-coordinate-alpha32-agreement',
              'independentGroundTruth': False, 'summary': stats, 'pairedCases': len(values),
              'inputReportSha256': [hashlib.sha256((folder / 'report.json').read_bytes()).hexdigest() for folder in (left, right)],
              'meanMaskAgreementIou': statistics.mean(values) if values else None, 'results': rows}
    output.mkdir(parents=True, exist_ok=False)
    write_json(output / 'comparison.json', result)
    document = '''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Sculpt mask comparison</title><style>
    :root{color-scheme:dark}body{font:14px system-ui;background:#171c19;color:#e1e7df;max-width:1400px;margin:36px auto;padding:0 24px}h1{font-size:28px;font-weight:500}h2{font-size:18px}h3{font-size:13px;font-weight:500}p{line-height:1.7;color:#aebcac}.row{display:grid;grid-template-columns:1fr 1fr 1fr;gap:22px}section{border-top:1px solid #3d483d;padding:20px 0}figure{margin:8px 0}img{width:100%;height:auto;max-height:420px;object-fit:contain;background:repeating-conic-gradient(#252c27 0% 25%,#394039 0% 50%) 50%/16px 16px}figcaption{font-size:12px;color:#aebcac;margin:8px 0}.failure{color:#e6b8a9}@media(max-width:800px){.row{grid-template-columns:1fr}}
    </style><h1>Sculpt · foreground comparison</h1><p>Both predictions use the same source coordinates. Agreement IoU measures how similarly the models select pixels. It does not establish which mask is correct; no independent annotations are supplied. Inspect lost parts and retained background. CPU session/model loading is included in worker time. Images stay in this offline report.</p>'''
    (output / 'index.html').write_text(document + '\n'.join(sections) + '</html>')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('left', 'right', 'output'): parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(compare(args.left, args.right, args.output)['summary'], indent=2))


if __name__ == '__main__': main()
