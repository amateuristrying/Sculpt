import pytest
from sculpt_backend.memory import check_memory


def test_high_quality_rejects_pressure_before_loading_model():
    with pytest.raises(ValueError, match='Close other apps or choose Draft'):
        check_memory('high', 4.2)
    assert check_memory('draft', 4.2)['resolution'] == 96


def test_balanced_accepts_sufficient_available_memory():
    assert check_memory('balanced', 5.0)['resolution'] == 128


def test_unknown_quality_does_not_default_to_large_allocation():
    with pytest.raises(ValueError, match='Unknown geometry'):
        check_memory('ultra', 64)


def test_memory_headroom_and_peak_estimate_are_not_counted_twice():
    assert check_memory('balanced', 3.2)['estimatedMemoryGb'] == 4.5
    with pytest.raises(ValueError, match='headroom'):
        check_memory('balanced', 2.8)
