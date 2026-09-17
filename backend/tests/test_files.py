import os

import pytest

from sculpt_backend.files import atomic_write_bytes


def test_atomic_write_publishes_complete_asset_and_removes_temporary_file(tmp_path):
    output = tmp_path / 'asset' / 'mesh.glb'
    atomic_write_bytes(output, b'complete new asset')
    assert output.read_bytes() == b'complete new asset'
    assert list(output.parent.iterdir()) == [output]


@pytest.mark.parametrize('failing_operation', ['fsync', 'replace'])
def test_interrupted_write_preserves_previous_asset(tmp_path, monkeypatch, failing_operation):
    output = tmp_path / 'mesh.glb'
    output.write_bytes(b'previous complete asset')

    def fail(*_args):
        raise OSError('disk failure')

    monkeypatch.setattr(os, failing_operation, fail)
    with pytest.raises(OSError, match='disk failure'):
        atomic_write_bytes(output, b'incomplete replacement')
    assert output.read_bytes() == b'previous complete asset'
    assert list(tmp_path.iterdir()) == [output]
