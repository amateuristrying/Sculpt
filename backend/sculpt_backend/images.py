from pathlib import Path
import os
import warnings


def prepare_image(source: Path, output: Path):
    import numpy as np
    from PIL import Image, ImageOps
    import rembg

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
    alpha = np.asarray(image.getchannel("A"))
    if alpha.min() >= 250:
        # Segment the actual image, not a guessed shape or filename class.
        if not (Path(os.environ.get("U2NET_HOME", "")) / "u2net.onnx").is_file():
            raise ValueError("Background model is missing. Run npm run backend:setup; generation never downloads models.")
        session = rembg.new_session("u2net", providers=["CPUExecutionProvider"])
        image = rembg.remove(image, session=session)
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
    gray = Image.new("RGBA", canvas.size, (128, 128, 128, 255))
    image = Image.alpha_composite(gray, canvas).convert("RGB").resize((512, 512), Image.Resampling.LANCZOS)
    image.save(output)
    return image
