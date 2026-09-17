import pytest

from sculpt_backend.triposr import extraction_chunk_size


def test_default_and_allowed_chunk_sizes():
    assert extraction_chunk_size() == 4096
    for value in (4096, 8192, 16384):
        assert extraction_chunk_size(value) == value


@pytest.mark.parametrize('value', [0, -1, True, '8192', 8192.0, 1000000])
def test_chunk_size_cannot_disable_memory_bounds(value):
    with pytest.raises(ValueError, match='chunk size'):
        extraction_chunk_size(value)
