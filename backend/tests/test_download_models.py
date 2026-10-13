from unittest.mock import Mock

import pytest
from requests import Response
from requests.exceptions import ChunkedEncodingError, HTTPError, Timeout

import download_models


@pytest.fixture
def retry_clock(monkeypatch):
    clock = Mock()
    monkeypatch.setattr(download_models.time, 'sleep', clock)
    return clock


def test_interrupted_download_reuses_pinned_request_and_partial_file(tmp_path, retry_clock):
    partial = tmp_path / 'weights.incomplete'
    partial.write_bytes(b'first part')
    attempts = []
    def download(repo, name, **kwargs):
        attempts.append((repo, name, kwargs))
        assert partial.read_bytes() == b'first part'
        if len(attempts) == 1:
            raise ChunkedEncodingError('Disconnected mid-body')
        return tmp_path / name
    result = download_models.download_with_retry(download, 'engine/repo', 'model.ckpt',
        revision='pinned-revision', local_dir=tmp_path, progress_value=58)
    assert result == tmp_path / 'model.ckpt'
    assert attempts[0] == attempts[1]
    retry_clock.assert_called_once_with(2)


def test_repeated_timeouts_are_bounded(retry_clock):
    download = Mock(side_effect=Timeout('Offline'))
    with pytest.raises(Timeout):
        download_models.download_with_retry(download, progress_value=50)
    assert download.call_count == 3
    assert [call.args[0] for call in retry_clock.call_args_list] == [2, 4]


@pytest.mark.parametrize('status,retried', [(401, False), (403, False), (404, False), (429, True), (503, True)])
def test_only_transient_http_errors_retry(status, retried, retry_clock):
    response = Response()
    response.status_code = status
    error = HTTPError('Download failed', response=response)
    download = Mock(side_effect=[error, 'ready'])
    if retried:
        assert download_models.download_with_retry(download, progress_value=58) == 'ready'
    else:
        with pytest.raises(HTTPError):
            download_models.download_with_retry(download, progress_value=58)
        retry_clock.assert_not_called()


def test_checksum_failure_is_not_retried(retry_clock):
    download = Mock(side_effect=ValueError('Checksum mismatch'))
    with pytest.raises(ValueError):
        download_models.download_with_retry(download, progress_value=92)
    assert download.call_count == 1
    retry_clock.assert_not_called()
