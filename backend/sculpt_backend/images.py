from pathlib import Path
import os
import warnings


def load_source(source: Path):
    """One coordinate system for preview, brush edits, and inference."""
    import numpy as np
    from PIL import Image, ImageOps
    Image.MAX_IMAGE_PIXELS = 40_000_000
    with warnings.catch_warnings():
        warnings.simplefilter("error", Image.DecompressionBombWarning)
        with Image.open(source) as opened:
            if opened.format not in {"PNG", "JPEG", "WEBP"}:
                raise ValueError("Choose a PNG, JPG, or WEBP source image.")
            if opened.width < 16 or opened.height < 16:
                raise ValueError("The source image is too small (minimum 16 × 16).")
            image = ImageOps.exif_transpose(opened).convert("RGBA")
    image.thumbnail((1024, 1024), Image.Resampling.LANCZOS)
    return image


def segment_image(image, background='auto'):
    import numpy as np
    import rembg

    if background not in {'auto', 'keep'}:
        raise ValueError('Unknown background mode.')
    alpha = np.asarray(image.getchannel("A"))
    if background == 'auto' and alpha.min() >= 250:
        # Segment the actual image, not a guessed shape or filename class.
        if not (Path(os.environ.get("U2NET_HOME", "")) / "u2net.onnx").is_file():
            raise ValueError("Background model is missing. Repair the local runtime in Sculpt; generation never downloads models.")
        session = rembg.new_session("u2net", providers=["CPUExecutionProvider"])
        # Keep straight RGB: the gray-background composite below applies alpha
        # exactly once. rembg's default naive_cutout premultiplies RGB, which
        # would darken soft edges a second time during alpha_composite.
        image = rembg.remove(image, session=session, putalpha=True)
    return image


def prepare_image(source: Path, output: Path, background: str = 'auto', mask_path: Path | None = None):
    import numpy as np
    from PIL import Image

    image = load_source(source)
    if mask_path is None:
        image = segment_image(image, background)
    else:
        # Use the approved mask verbatim; never run automatic removal again.
        if mask_path.stat().st_size > 5 * 1024 * 1024:
            raise ValueError('The foreground mask exceeds its size limit.')
        with Image.open(mask_path) as mask:
            if mask.format != 'PNG' or mask.size != image.size or mask.mode != 'L':
                raise ValueError('The foreground mask must be a grayscale PNG in source preview coordinates.')
            image.putalpha(mask)
    data = np.asarray(image)
    foreground = data[:, :, 3] > 32
    if int(foreground.sum()) < 64:
        raise ValueError("No foreground object was found. Try a clearer photograph.")
    ys, xs = np.nonzero(foreground)
    crop = image.crop((int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1))
    edge = max(32, int(max(crop.size) / 0.85))
    canvas = Image.new("RGBA", (edge, edge), (128, 128, 128, 0))
    canvas.paste(crop, ((edge - crop.width) // 2, (edge - crop.height) // 2))
    canvas.save(output.parent / "foreground.png")
    image.getchannel("A").save(output.parent / "source-mask.png")
    gray = Image.new("RGBA", canvas.size, (128, 128, 128, 255))
    image = Image.alpha_composite(gray, canvas).convert("RGB").resize((512, 512), Image.Resampling.LANCZOS)
    image.save(output)
    return image
