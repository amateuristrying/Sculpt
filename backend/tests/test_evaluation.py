import copy
import json
from pathlib import Path

import numpy as np
from PIL import Image
import pytest
import trimesh

from sculpt_eval.dataset import DEFAULT_MANIFEST, fetch_photos, load_manifest, sha256, verify_photo
from sculpt_eval.render import DEFAULT_CAMERA, iou, load_mesh, project, render
from sculpt_eval.report import compare, make_reference, read_run


def fixture_run(tmp_path, name='baseline'):
    folder = tmp_path / name
    folder.mkdir()
    job = folder / 'box'
    job.mkdir()
    mesh = trimesh.creation.box(extents=[.8, .8, .8])
    mesh.export(job / 'mesh.glb')
    photo = job / 'source.png'
    Image.new('RGB', (64, 64), 'red').save(photo)
    _, mask = render(load_mesh(job / 'mesh.glb'), DEFAULT_CAMERA)
    pixels = np.full((256, 256, 4), 180, dtype=np.uint8)
    pixels[:, :, 3] = mask * 255
    Image.fromarray(pixels).save(job / 'foreground.png')
    Image.fromarray(pixels[:, :, :3]).save(job / 'input.png')
    (job / 'request.json').write_text(json.dumps({'sourcePath': str(photo)}))
    dataset = {'schemaVersion': 1, 'cases': [{'id': 'box', 'source': str(photo), 'sha256': sha256(photo),
               'category': 'tools', 'license': 'CC0-1.0', 'sourceUrl': 'https://example.test/photo',
               'licenseEvidence': {'test': True}, 'author': 'A < B'}]}
    manifest = folder / 'photos.json'
    manifest.write_text(json.dumps(dataset))
    data = {'schemaVersion': 1, 'dataset': dataset, 'datasetSha256': sha256(manifest),
            'results': [{'id': 'box', 'sourceSha256': sha256(photo), 'validResult': True,
                         'metrics': {'totalSeconds': 2, 'faces': 12, 'vertices': 8, 'peakProcessMemoryMb': 100}}]}
    (folder / 'report.json').write_text(json.dumps(data))
    return folder, data


def test_iou_hand_computed_and_empty():
    assert iou([[1, 0], [1, 0]], [[1, 1], [0, 0]]) == pytest.approx(1 / 3)
    assert iou([[0]], [[0]]) is None
    assert iou([[1]], [[0]]) == 0
    with pytest.raises(ValueError, match='dimensions'):
        iou([[1]], [[1, 1]])


def test_perspective_camera_and_pixel_center_rasterization():
    # At azimuth 0, look from +X: GLB -Z projects right, +Y projects up.
    xy, depth, _ = project(np.array([[0., 0, 0], [0, .2, 0], [0, 0, -.2]]), DEFAULT_CAMERA, 100)
    assert np.allclose(xy[0], [50, 50])
    assert xy[1, 1] < 50 and xy[2, 0] > 50
    assert np.allclose(depth, 1.9)
    with pytest.raises(ValueError, match='near plane'):
        project(np.array([[2., 0, 0]]), DEFAULT_CAMERA, 100)


def test_z_buffer_keeps_nearer_triangle_independent_of_face_order():
    vertices = np.array([[0, -.5, -.5], [0, .5, -.5], [0, 0, .5],
                         [.2, -.5, -.5], [.2, .5, -.5], [.2, 0, .5]])
    faces = np.array([[0, 1, 2], [3, 4, 5]])
    colors = np.array([[1., 0, 0]] * 3 + [[0, 1., 0]] * 3)
    first, mask = render((vertices, faces, colors), size=64)
    second, other_mask = render((vertices, faces[::-1], colors), size=64)
    assert np.array_equal(first, second)
    assert np.array_equal(mask, other_mask)
    assert np.asarray(first)[32, 32, 1] > np.asarray(first)[32, 32, 0]


def test_scene_node_transforms_are_applied(tmp_path):
    scene = trimesh.Scene()
    scene.add_geometry(trimesh.creation.box(), transform=trimesh.transformations.translation_matrix([0, 2, 0]))
    path = tmp_path / 'transformed.glb'
    scene.export(path)
    vertices, _, _ = load_mesh(path)
    assert vertices[:, 1].mean() == pytest.approx(2)


def test_dataset_rejects_restricted_license_duplicate_photos_and_changed_bytes(tmp_path):
    path, data = fixture_run(tmp_path)
    manifest = path / 'photos.json'
    case = load_manifest(manifest)['cases'][0]
    assert verify_photo(case, manifest).is_file()
    Path(case['source']).write_bytes(b'changed')
    with pytest.raises(ValueError, match='changed photo'):
        verify_photo(case, manifest)
    dataset = data['dataset']
    dataset['cases'][0]['license'] = 'CC-BY-NC-4.0'
    manifest.write_text(json.dumps(dataset))
    with pytest.raises(ValueError, match='CC0'):
        load_manifest(manifest)
    dataset['cases'][0]['license'] = 'CC0-1.0'
    duplicate = copy.deepcopy(dataset['cases'][0])
    duplicate['id'] = 'another-box'
    dataset['cases'].append(duplicate)
    manifest.write_text(json.dumps(dataset))
    with pytest.raises(ValueError, match='unique SHA'):
        load_manifest(manifest)


def test_fetch_pins_bytes_and_does_not_leave_a_corrupt_photo(tmp_path, monkeypatch):
    import io
    import urllib.request
    baseline, data = fixture_run(tmp_path)
    case = data['dataset']['cases'][0]
    case['downloadUrl'] = 'https://example.test/photo.png'
    manifest = baseline / 'photos.json'
    manifest.write_text(json.dumps(data['dataset']))
    source = Path(case['source'])
    original = source.read_bytes()
    source.unlink()
    monkeypatch.setattr('sculpt_eval.dataset.time.sleep', lambda _: None)
    monkeypatch.setattr(urllib.request, 'urlopen', lambda *args, **kwargs: io.BytesIO(b'changed source'))
    with pytest.raises(ValueError, match='differs from pinned photo'):
        fetch_photos(manifest)
    assert not source.exists()
    monkeypatch.setattr(urllib.request, 'urlopen', lambda *args, **kwargs: io.BytesIO(original))
    fetch_photos(manifest)
    assert source.read_bytes() == original
    assert not source.with_suffix('.png.part').exists()


def test_comparison_detects_geometry_regression_without_refitting_camera(tmp_path):
    baseline, data = fixture_run(tmp_path)
    reference = tmp_path / 'reference'
    frozen = make_reference(baseline, reference)
    original_reference = (reference / 'reference.json').read_bytes()
    candidate = tmp_path / 'candidate'
    (candidate / 'box').mkdir(parents=True)
    smaller = trimesh.creation.box(extents=[.3, .3, .3])
    smaller.export(candidate / 'box/mesh.glb')
    (candidate / 'report.json').write_text(json.dumps(data))
    result = compare(baseline, candidate, reference, tmp_path / 'comparison')
    assert result['results'][0]['left']['iou'] > .85
    assert result['results'][0]['deltaIou'] < -.5
    assert result['results'][0]['camera'] == frozen['cases'][0]['camera']
    assert (reference / 'reference.json').read_bytes() == original_reference
    html = (tmp_path / 'comparison/index.html').read_text()
    assert 'A &lt; B' in html and 'data:image/png;base64,' in html
    assert '+270°' in html and 'not human ground truth' in html
    self_check = compare(candidate, candidate, reference, tmp_path / 'self')
    assert self_check['summary']['meanPairedDeltaIou'] == 0


def test_comparison_keeps_failed_cases_and_rejects_mismatched_dataset_or_reference(tmp_path):
    baseline, data = fixture_run(tmp_path)
    reference = tmp_path / 'reference'
    make_reference(baseline, reference)
    candidate = tmp_path / 'candidate'
    candidate.mkdir()
    data['results'][0].update(validResult=False, error='No foreground')
    (candidate / 'report.json').write_text(json.dumps(data))
    report = compare(baseline, candidate, reference, tmp_path / 'failures')
    assert report['summary']['cases'] == 1
    assert report['summary']['scoredPairs'] == 0
    assert report['summary']['right']['meanIouFailuresAsZero'] == 0
    data['datasetSha256'] = 'a' * 64
    (candidate / 'report.json').write_text(json.dumps(data))
    with pytest.raises(ValueError, match='identical hashed dataset'):
        compare(baseline, candidate, reference, tmp_path / 'mismatch')
    (reference / 'box/mask.png').write_bytes(b'damaged')
    with pytest.raises(ValueError, match='reference image changed'):
        compare(baseline, baseline, reference, tmp_path / 'damaged')


def test_run_reader_rejects_source_substitution(tmp_path):
    baseline, data = fixture_run(tmp_path)
    data['results'][0]['sourceSha256'] = 'a' * 64
    (baseline / 'report.json').write_text(json.dumps(data))
    with pytest.raises(ValueError, match='source hashes'):
        read_run(baseline)


def test_insufficient_memory_stops_before_creating_a_run(tmp_path, monkeypatch):
    from sculpt_eval.runner import run
    from types import SimpleNamespace
    import psutil
    baseline, _ = fixture_run(tmp_path)
    monkeypatch.setattr(psutil, 'virtual_memory', lambda: SimpleNamespace(available=2 * 1024 ** 3))
    with pytest.raises(ValueError, match='headroom'):
        run(baseline / 'photos.json', tmp_path / 'should-not-exist', 'balanced')
    assert not (tmp_path / 'should-not-exist').exists()


def test_committed_photo_set_has_required_coverage_and_no_upstream_sources():
    data = load_manifest(DEFAULT_MANIFEST)
    assert 30 <= len(data['cases']) <= 40
    assert {case['category'] for case in data['cases']} >= {'shoes', 'mugs', 'toys', 'plants', 'tools', 'furniture', 'food', 'reflective', 'transparent', 'clutter'}
    assert all(case['license'] == 'CC0-1.0' and 'TripoSR' not in case['downloadUrl'] for case in data['cases'])
    assert all(case['licenseEvidence']['licenseShortName'] == 'CC0' for case in data['cases'])
