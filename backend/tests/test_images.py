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


def test_keep_background_bypasses_segmentation(tmp_path, monkeypatch):
    monkeypatch.setenv("U2NET_HOME", str(tmp_path / "missing"))
    source = tmp_path / 'object.png'
    Image.new('RGB', (100, 100), (230, 180, 30)).save(source)
    result = prepare_image(source, tmp_path / 'input.png', 'keep')
    assert result.getpixel((256, 256)) == (230, 180, 30)


@pytest.mark.parametrize("kind", ["tiny", "corrupt", "gif"])
def test_invalid_input_is_rejected(tmp_path, kind):
    source = tmp_path / "invalid"
    if kind == "corrupt":
        source.write_bytes(b"not an image")
    else:
        Image.new("RGB", (2, 2) if kind == "tiny" else (32, 32)).save(source, format="PNG" if kind == "tiny" else "GIF")
    with pytest.raises((ValueError, OSError)):
        prepare_image(source, tmp_path / "input.png")


def test_preview_mask_round_trip_preserves_prepared_pixels(tmp_path):
    from sculpt_backend.images import load_source
    source = tmp_path / 'alpha.png'
    rgba = np.zeros((80, 120, 4), dtype=np.uint8)
    rgba[:, :, :3] = [239, 193, 43]
    rgba[15:65, 30:100, 3] = 192
    Image.fromarray(rgba).save(source)
    direct, reviewed = tmp_path / 'direct', tmp_path / 'reviewed'
    direct.mkdir(); reviewed.mkdir()
    expected = prepare_image(source, direct / 'input.png')
    mask = tmp_path / 'mask.png'
    load_source(source).getchannel('A').save(mask)
    actual = prepare_image(source, reviewed / 'input.png', mask_path=mask)
    assert np.array_equal(np.asarray(expected), np.asarray(actual))
    assert (direct / 'foreground.png').read_bytes() == (reviewed / 'foreground.png').read_bytes()


def test_corrected_mask_bypasses_segmentation_and_changes_input(tmp_path, monkeypatch):
    from sculpt_backend import images
    source = tmp_path / 'source.png'
    pixels = np.full((80, 120, 3), 200, dtype=np.uint8)
    pixels[20:60, 10:40] = [240, 10, 10]
    pixels[20:60, 70:100] = [10, 20, 240]
    Image.fromarray(pixels).save(source)
    def forbidden(*args): raise AssertionError('Approved masks must not be segmented again')
    monkeypatch.setattr(images, 'segment_image', forbidden)
    for side, x, color in [('left', 10, (240, 10, 10)), ('right', 70, (10, 20, 240))]:
        folder = tmp_path / side; folder.mkdir()
        alpha = np.zeros((80, 120), dtype=np.uint8); alpha[20:60, x:x + 30] = 255
        path = folder / 'mask.png'; Image.fromarray(alpha).save(path)
        result = prepare_image(source, folder / 'input.png', mask_path=path)
        assert result.getpixel((256, 256)) == color


def test_mask_coordinates_follow_exif_orientation(tmp_path):
    from sculpt_backend.images import load_source
    image = Image.new('RGB', (40, 80), 'yellow')
    exif = image.getexif(); exif[274] = 6
    source = tmp_path / 'rotated.jpg'; image.save(source, exif=exif)
    assert load_source(source).size == (80, 40)
    mask = tmp_path / 'mask.png'; Image.new('L', (40, 80), 255).save(mask)
    with pytest.raises(ValueError, match='coordinates'):
        prepare_image(source, tmp_path / 'input.png', mask_path=mask)
    Image.new('L', (80, 40), 255).save(mask)
    assert prepare_image(source, tmp_path / 'input.png', mask_path=mask).size == (512, 512)


def test_empty_prediction_can_be_previewed_and_painted_before_generation(tmp_path):
    import hashlib
    from sculpt_backend.masks import prepare_mask
    source = tmp_path / 'empty.png'
    Image.new('RGBA', (40, 40), (220, 180, 0, 0)).save(source)
    output = tmp_path / 'preview'
    metrics = prepare_mask({'sourcePath': str(source), 'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                           'outputPath': str(output)}, lambda *args: None)
    assert metrics['width'] == 40
    assert Image.open(output / 'mask.png').getextrema() == (0, 0)
    assert Image.open(output / 'preview.png').getpixel((20, 20)) == (220, 180, 0)
    Image.new('L', (40, 40), 255).save(output / 'mask.png')
    assert prepare_image(source, tmp_path / 'input.png', mask_path=output / 'mask.png').getpixel((256, 256)) == (220, 180, 0)


def test_automatic_mask_does_not_apply_alpha_to_color_twice(tmp_path, monkeypatch):
    import rembg
    source = tmp_path / 'source.png'; Image.new('RGB', (40, 40), (240, 200, 20)).save(source)
    monkeypatch.setenv('U2NET_HOME', str(tmp_path)); (tmp_path / 'u2net.onnx').write_bytes(b'test')
    monkeypatch.setattr(rembg, 'new_session', lambda *a, **kw: None)
    def remove(image, session=None, **kwargs):
        assert kwargs.get('putalpha') is True
        image.putalpha(Image.new('L', image.size, 128)); return image
    monkeypatch.setattr(rembg, 'remove', remove)
    result = prepare_image(source, tmp_path / 'input.png')
    # 50% yellow + 50% gray, not 25% yellow + 50% gray.
    assert result.getpixel((256, 256)) == (184, 164, 74)
