"""Frozen-reference silhouette comparisons and self-contained, offline HTML."""
import base64
import copy
from html import escape
import io
import json
from pathlib import Path
import statistics

import numpy as np
from PIL import Image, ImageOps

from .dataset import sha256
from .render import DEFAULT_CAMERA, iou, load_mesh, mask_overlay, render, silhouette
from .runner import write_json

SIZE = 256
METHOD = 'frozen-baseline-camera-u2net-alpha32-v1'


def read_run(path):
    data = json.loads((path / 'report.json').read_text())
    if data.get('schemaVersion') != 1 or 'datasetSha256' not in data:
        raise ValueError('Use an evaluation run, not the older showcase benchmark format')
    expected = {case['id']: case['sha256'] for case in data['dataset']['cases']}
    ids = [row['id'] for row in data['results']]
    if len(set(ids)) != len(ids) or any(name not in expected for name in ids):
        raise ValueError('Run contains duplicate or unknown cases')
    if any(row.get('sourceSha256') != expected[row['id']] for row in data['results']):
        raise ValueError('Run source hashes disagree with its dataset')
    return data


def fit_camera(mesh, target):
    """Baseline-only coarse coordinate search; an estimated view, NOT calibrated optics.

    TripoSR does not return the input camera. Search the baseline once; all future
    candidates must use this exact camera, with no mirroring or mesh normalization.
    """
    small = np.asarray(Image.fromarray(target).resize((64, 64), Image.Resampling.NEAREST))
    camera = dict(DEFAULT_CAMERA)
    for field, values in [('azimuth', range(0, 360, 45)), ('elevation', [-15, 0, 15, 30]),
                          ('distance', [1.6, 1.9, 2.2, 2.5])]:
        candidates = [{**camera, field: float(value)} for value in values]
        camera = max(candidates, key=lambda candidate: iou(small, silhouette(mesh, candidate, 64)) or 0)
    azimuth = camera['azimuth']
    camera = max([{**camera, 'azimuth': float((azimuth + delta) % 360)} for delta in [-15, 0, 15]],
                 key=lambda candidate: iou(small, silhouette(mesh, candidate, 64)) or 0)
    return camera


def make_reference(run_path, output):
    run_path = run_path.resolve()
    data = read_run(run_path)
    if len(data['results']) != len(data['dataset']['cases']):
        raise ValueError('Finish the baseline run before freezing its reference')
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    rows = {row['id']: row for row in data['results']}
    reference = {'schemaVersion': 1, 'method': METHOD, 'size': SIZE,
                 'datasetSha256': data['datasetSha256'], 'baselineReportSha256': sha256(run_path / 'report.json'),
                 'maskKind': 'Baseline foreground (U2Net or existing source alpha), threshold > 32; not human ground truth',
                 'cameraKind': 'Approximate photo view fitted once on the baseline; frozen for every comparison',
                 'cases': []}
    for case in data['dataset']['cases']:
        print(f"Freezing mask and photo-view estimate: {case['id']}", flush=True)
        folder = output / case['id']
        folder.mkdir()
        source_folder = run_path / case['id']
        row = {'id': case['id'], 'sourceSha256': case['sha256'], 'camera': None, 'maskKind': 'Automatic U2Net'}
        source = None
        request = source_folder / 'request.json'
        if request.exists():
            source = Path(json.loads(request.read_text())['sourcePath'])
        if source is not None and source.is_file() and sha256(source) == case['sha256']:
            with Image.open(source) as photo:
                if photo.convert('RGBA').getchannel('A').getextrema()[0] < 250:
                    row['maskKind'] = 'Existing source alpha'
                ImageOps.contain(ImageOps.exif_transpose(photo).convert('RGB'), (512, 512)).save(folder / 'photo.jpg')
            row['photoSha256'] = sha256(folder / 'photo.jpg')
        foreground = source_folder / 'foreground.png'
        if foreground.exists():
            with Image.open(foreground) as image:
                # Match the worker's alpha cutoff before nearest-neighbor scaling.
                alpha = np.asarray(image.convert('RGBA'))[:, :, 3] > 32
            mask_image = Image.fromarray(alpha).resize((SIZE, SIZE), Image.Resampling.NEAREST)
            mask_image.save(folder / 'mask.png')
            row['maskSha256'] = sha256(folder / 'mask.png')
            with Image.open(source_folder / 'input.png') as image:
                image.resize((SIZE, SIZE), Image.Resampling.LANCZOS).save(folder / 'input.png')
            row['inputSha256'] = sha256(folder / 'input.png')
            if rows[case['id']]['validResult']:
                row['camera'] = fit_camera(load_mesh(source_folder / 'mesh.glb'), np.asarray(mask_image))
        if row['camera'] is None:
            row['unscoredReason'] = 'Baseline did not produce a mesh and foreground mask; inspect this failure separately'
        reference['cases'].append(row)
        write_json(output / 'reference.json', reference)
    return reference


def data_image(image):
    stream = io.BytesIO()
    image.save(stream, format='PNG')
    return 'data:image/png;base64,' + base64.b64encode(stream.getvalue()).decode()


def img(image, label):
    return f'<figure><img loading="lazy" src="{data_image(image)}" alt="{escape(label)}"><figcaption>{escape(label)}</figcaption></figure>'


def number(value, digits=2):
    return '—' if value is None else f'{value:,.{digits}f}'


def render_result(run_path, result, reference, target):
    if result is None or not result.get('validResult'):
        error = (result or {}).get('error', 'Case not completed')
        return {'status': 'failed', 'iou': None, 'error': error}, f'<p class="failure">{escape(str(error))}</p>'
    metrics = result.get('metrics') or {}
    score = {'status': 'ok', 'iou': None, 'wallSeconds': result.get('wallSeconds'),
             **{key: metrics.get(key) for key in ['device', 'maskSha256', 'totalSeconds', 'peakProcessMemoryMb', 'availableMemoryGbAtStart', 'faces', 'vertices']}}
    try:
        mesh = load_mesh(run_path / result['id'] / 'mesh.glb')
        camera = reference['camera'] or DEFAULT_CAMERA
        views = []
        for delta in [0, 90, 180, 270]:
            image, outline = render(mesh, {**camera, 'azimuth': camera['azimuth'] + delta}, SIZE)
            views.append(img(image, 'Estimated photo view' if delta == 0 else f'+{delta}°'))
            if delta == 0 and target is not None and reference['camera']:
                score['iou'] = iou(target, outline)
                overlay = mask_overlay(target, outline)
        label = f'IoU {number(score["iou"], 3)} · {number(score["totalSeconds"])} s · {number(score["faces"], 0)} faces'
        body = f'<p class="metrics">{label}</p>'
        input_path = run_path / result['id'] / 'input.png'
        if input_path.is_file():
            score['inputSha256'] = sha256(input_path)
            with Image.open(input_path) as image:
                preview = image.copy(); preview.thumbnail((256, 256))
                body += '<details><summary>Input used for this mesh</summary>' + img(preview, 'Actual cropped model input') + '</details>'
        body += '<div class="views">' + ''.join(views) + '</div>'
        if score['iou'] is not None:
            body += '<details><summary>Silhouette overlap</summary>' + img(overlay, 'White: overlap · blue: mask only · orange: mesh only') + '</details>'
        return score, body
    except Exception as error:
        return {**score, 'status': 'render-error', 'iou': None, 'error': str(error)}, f'<p class="failure">Render failed: {escape(str(error))}</p>'


def summarize(rows):
    result = {'cases': len(rows), 'scoredPairs': sum(row['deltaIou'] is not None for row in rows)}
    for side in ['left', 'right']:
        scores = [row[side]['iou'] for row in rows if row[side]['iou'] is not None]
        pairs = [row[side]['iou'] for row in rows if row['deltaIou'] is not None]
        times = [row[side]['totalSeconds'] for row in rows if row[side].get('totalSeconds') is not None]
        memory = [row[side]['peakProcessMemoryMb'] for row in rows if row[side].get('peakProcessMemoryMb') is not None]
        faces = [row[side]['faces'] for row in rows if row[side].get('faces') is not None]
        result[side] = {'successfulRenders': sum(row[side]['status'] == 'ok' for row in rows),
                        'scoredCases': len(scores), 'meanIou': statistics.mean(scores) if scores else None,
                        'pairedMeanIou': statistics.mean(pairs) if pairs else None,
                        'meanIouFailuresAsZero': sum(scores) / len(rows) if rows else None,
                        'medianSeconds': statistics.median(times) if times else None,
                        'medianFaces': statistics.median(faces) if faces else None,
                        'maxProcessRssMb': max(memory) if memory else None}
    deltas = [row['deltaIou'] for row in rows if row['deltaIou'] is not None]
    result['meanPairedDeltaIou'] = statistics.mean(deltas) if deltas else None
    return result


def compare(left_path, right_path, reference_path, output):
    left, right = read_run(left_path), read_run(right_path)
    reference = json.loads((reference_path / 'reference.json').read_text())
    if reference.get('schemaVersion') != 1 or reference.get('method') != METHOD or reference.get('size') != SIZE:
        raise ValueError('Unsupported evaluation reference')
    if len({left['datasetSha256'], right['datasetSha256'], reference['datasetSha256']}) != 1:
        raise ValueError('Both runs and reference must use the identical hashed dataset')
    case_ids = [case['id'] for case in left['dataset']['cases']]
    if sorted(row['id'] for row in reference['cases']) != sorted(case_ids):
        raise ValueError('Reference must include each dataset case exactly once')
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    lookup = [{row['id']: row for row in run['results']} for run in [left, right]]
    references = {row['id']: row for row in reference['cases']}
    rows, sections = [], []
    for case in left['dataset']['cases']:
        print(f"Comparing four views: {case['id']}", flush=True)
        ref = references[case['id']]
        if ref['sourceSha256'] != case['sha256']:
            raise ValueError('Reference source does not match dataset')
        folder = reference_path / case['id']
        target, source_html = None, ''
        for name, field in [('photo.jpg', 'photoSha256'), ('input.png', 'inputSha256'), ('mask.png', 'maskSha256')]:
            if field not in ref:
                continue
            path = folder / name
            if sha256(path) != ref[field]:
                raise ValueError('Frozen reference image changed: ' + str(path))
            with Image.open(path) as image:
                if name == 'mask.png':
                    target = np.asarray(image.convert('L')) > 0
                else:
                    source_html += img(image, 'Source photo' if name == 'photo.jpg' else 'Baseline model input')
        a, html_a = render_result(left_path, lookup[0].get(case['id']), ref, target)
        if left_path.resolve() == right_path.resolve():
            b, html_b = copy.deepcopy(a), html_a
        else:
            b, html_b = render_result(right_path, lookup[1].get(case['id']), ref, target)
        delta = b['iou'] - a['iou'] if a['iou'] is not None and b['iou'] is not None else None
        rows.append({'id': case['id'], 'category': case['category'], 'sourceSha256': case['sha256'],
                     'camera': ref['camera'], 'left': a, 'right': b, 'deltaIou': delta})
        credit = f'{escape(case["author"] or "Photographer not specified")} · {escape(case["license"])}'
        sections.append(f'<section><h2>{escape(case["id"])} <small>{escape(case["category"])} · Δ IoU {number(delta, 3)}</small></h2>'
                        f'<div class="comparison"><div>{source_html}<p>{escape(ref.get("maskKind", "Baseline foreground"))}</p><p>{credit}</p><a href="{escape(case["sourceUrl"], quote=True)}">Source &amp; license</a></div>'
                        f'<div><h3>Before · {escape(left_path.name)}</h3>{html_a}</div><div><h3>After · {escape(right_path.name)}</h3>{html_b}</div></div></section>')
    summary = summarize(rows)
    report = {'schemaVersion': 1, 'method': METHOD, 'datasetSha256': left['datasetSha256'],
              'referenceSha256': sha256(reference_path / 'reference.json'),
              'leftReportSha256': sha256(left_path / 'report.json'), 'rightReportSha256': sha256(right_path / 'report.json'),
              'leftRun': left_path.name, 'rightRun': right_path.name,
              'leftDevice': left.get('device'), 'rightDevice': right.get('device'),
              'summary': summary, 'categories': {category: summarize([r for r in rows if r['category'] == category]) for category in sorted({r['category'] for r in rows})},
              'results': rows}
    write_json(output / 'comparison.json', report)
    stats = ''.join(f'<div><h3>{side.title()}</h3><p>{summary[side]["successfulRenders"]}/{len(rows)} rendered · '
                    f'paired IoU {number(summary[side]["pairedMeanIou"], 3)}<br>'
                    f'{number(summary[side]["medianSeconds"])} s median · {number(summary[side]["maxProcessRssMb"], 0)} MB max RSS</p></div>' for side in ['left', 'right'])
    html = '<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Sculpt · Evaluation</title>' + STYLE
    html += f'<main><header><p class="eyebrow">SCULPT / LOCAL EVALUATION</p><h1>Real photos. Visible failure cases.</h1><p>{len(rows)} CC0 photographs · TripoSR · {escape(left.get("quality", "unknown"))} → {escape(right.get("quality", "unknown"))}<br>{escape(left_path.name)} → {escape(right_path.name)}</p>'
    html += f'<p>Worker device: {escape(left.get("device", "unknown"))} → {escape(right.get("device", "unknown"))}</p>'
    html += '<p class="note">IoU measures overlap with the frozen baseline mask (U2Net or source alpha), not human ground truth or 3D accuracy. The approximate photo camera is fitted once on the baseline and reused unchanged. A wrong mask can give a high score to a wrong object. Inspect all four views. RSS excludes total system/GPU memory pressure.</p>'
    html += '<div class="stats">' + stats + f'<div><h3>Paired change</h3><p>{number(summary["meanPairedDeltaIou"], 3)} IoU<br>{summary["scoredPairs"]} comparable cases</p></div></div></header>'
    html += ''.join(sections) + '</main></html>'
    (output / 'index.html').write_text(html)
    print(f"Comparison: {output / 'index.html'}", flush=True)
    return report


STYLE = '''<style>
*{box-sizing:border-box}body{margin:0;background:#121418;color:#e2e5ea;font:14px/1.6 -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif}
main{max-width:1500px;margin:auto;padding:48px 32px}header{max-width:1100px;margin-bottom:48px}h1{font-weight:500;letter-spacing:-1px;font-size:36px}
h2{font-weight:500;font-size:20px}h3{font-size:14px;font-weight:500}small,figcaption,.note{color:#9fa8b6}small{font-size:13px;margin-left:16px}
.eyebrow{letter-spacing:3px;font-size:12px;color:#adbbcf}.note{max-width:900px}.stats{display:flex;gap:64px}.comparison{display:grid;grid-template-columns:1fr 2fr 2fr;gap:24px}
section{border-top:1px solid #333943;padding:24px 0 40px}.views{display:grid;grid-template-columns:1fr 1fr;gap:12px}figure{margin:0 0 12px}img{width:100%;display:block;object-fit:contain;background:#1b1e23;border-radius:6px}
figcaption{font-size:12px;margin-top:4px}a{color:#96c9f2}.failure{color:#ffb094}.metrics{font-variant-numeric:tabular-nums;color:#c5d3e7}summary{cursor:pointer;color:#b7c7da}details img{max-width:256px;margin-top:12px}
@media(max-width:800px){main{padding:24px 14px}.comparison{grid-template-columns:1fr 1fr}.comparison>div:first-child{grid-column:1/-1;display:flex;flex-wrap:wrap;gap:12px}.comparison>div:first-child figure{width:140px}.stats{gap:20px}small{display:block;margin:0}}
</style>'''
