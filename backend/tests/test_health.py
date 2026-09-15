import json
from sculpt_backend.config import SPEC
from sculpt_backend.health import validate_install, checksum, LOCKFILE


def installed_fixture(root):
    files = {}
    for name in [*SPEC['artifacts'], 'TripoSR/tsr/system.py', 'venv/bin/python']:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b'test fixture')
        stat = path.stat()
        files[name] = {'size': stat.st_size, 'modifiedNs': stat.st_mtime_ns}
    manifest = {'version': SPEC['version'], 'modelRevision': SPEC['modelRevision'], 'sourceRevision': SPEC['sourceRevision'], 'files': files, 'dependencyLockSha256': checksum(LOCKFILE)}
    (root / 'ready.json').write_text(json.dumps(manifest))
    return manifest


def test_ready_marker_alone_is_not_healthy(tmp_path):
    (tmp_path / 'ready.json').write_text('{}')
    assert not validate_install(tmp_path)


def test_changed_weights_require_repair(tmp_path):
    installed_fixture(tmp_path)
    assert validate_install(tmp_path)
    (tmp_path / 'models/triposr/model.ckpt').write_bytes(b'damaged')
    assert not validate_install(tmp_path)


def test_missing_encoder_or_source_is_detected(tmp_path):
    installed_fixture(tmp_path)
    (tmp_path / 'TripoSR/tsr/system.py').unlink()
    assert not validate_install(tmp_path)


def test_invalid_manifest_path_is_rejected(tmp_path):
    manifest = installed_fixture(tmp_path)
    manifest['files']['../outside'] = {'size': 0, 'modifiedNs': 0}
    (tmp_path / 'ready.json').write_text(json.dumps(manifest))
    assert not validate_install(tmp_path)


def test_checksum_detects_content_change(tmp_path):
    file = tmp_path / 'weights'
    file.write_bytes(b'abc')
    assert checksum(file) == 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'


def test_dependency_change_requires_verification(tmp_path):
    manifest = installed_fixture(tmp_path)
    manifest['dependencyLockSha256'] = 'old dependency set'
    (tmp_path / 'ready.json').write_text(json.dumps(manifest))
    assert not validate_install(tmp_path)
