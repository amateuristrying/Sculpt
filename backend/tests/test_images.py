import numpy as np
from PIL import Image
import pytest
from sculpt_backend.images import prepare_image


def test_transparent_source_keeps_foreground_colors_and_normalizes(tmp_path):
    pixels = np.zeros((100, 200, 4), dtype=np.uint8)
    pixels[30:70, 20:180] = [250, 190, 20, 255]
    source = tmp_path / "source.png"
    Image.fromarray(pixels).save(source)
    result = prepare_image(source, tmp_path / "input.png")
    assert result.size == (512, 512)
    assert result.mode == "RGB"
    assert result.getpixel((256, 256)) == (250, 190, 20)
    assert result.getpixel((0, 0)) == (128, 128, 128)
    assert (tmp_path / "foreground.png").is_file()


def test_empty_foreground_is_an_error(tmp_path):
    source = tmp_path / "empty.png"
    Image.new("RGBA", (100, 100), (0, 0, 0, 0)).save(source)
    with pytest.raises(ValueError, match="No foreground"):
        prepare_image(source, tmp_path / "input.png")


def test_missing_segmentation_model_does_not_attempt_a_download(tmp_path, monkeypatch):
    monkeypatch.setenv("U2NET_HOME", str(tmp_path / "missing"))
    source = tmp_path / "image.jpg"
    Image.new("RGB", (100, 100), "yellow").save(source)
    with pytest.raises(ValueError, match="never downloads"):
        prepare_image(source, tmp_path / "input.png")


@pytest.mark.parametrize("kind", ["tiny", "corrupt", "gif"])
def test_invalid_input_is_rejected(tmp_path, kind):
    source = tmp_path / "invalid"
    if kind == "corrupt":
        source.write_bytes(b"not an image")
    else:
        Image.new("RGB", (2, 2) if kind == "tiny" else (32, 32)).save(source, format="PNG" if kind == "tiny" else "GIF")
    with pytest.raises((ValueError, OSError)):
        prepare_image(source, tmp_path / "input.png")
