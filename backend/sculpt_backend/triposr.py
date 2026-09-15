import json
import os
from pathlib import Path
import resource
import sys
import time

from .config import MODEL_REVISION, UPSTREAM_REVISION, SPEC

RESOLUTIONS = {profile['id']: profile['resolution'] for profile in SPEC['qualities']}


def generate(request: dict, root: Path, emit) -> dict:
    import numpy as np
    import torch

    from .images import prepare_image
    from .marching import install_cpu_operator
    from .memory import check_memory

    started = time.monotonic()
    requested_device = request.get("device", "auto")
    if requested_device not in {"auto", "mps", "cpu"}:
        raise ValueError("Unsupported compute device.")
    device = "mps" if requested_device == "auto" and torch.backends.mps.is_available() else requested_device
    if device == "auto":
        device = "cpu"
    if device == "mps" and not torch.backends.mps.is_available():
        raise ValueError("The PyTorch Metal backend is unavailable on this machine.")
    torch.set_num_threads(min(4, os.cpu_count() or 1))
    if device == "mps":
        torch.mps.set_per_process_memory_fraction(0.65)
    source = Path(request["sourcePath"])
    output = Path(request["outputPath"])
    output.parent.mkdir(parents=True, exist_ok=True)
    quality = request.get("quality", "draft")
    if quality not in RESOLUTIONS:
        raise ValueError("Unknown geometry quality.")
    import psutil
    available_gb = psutil.virtual_memory().available / (1024**3)
    check_memory(quality, available_gb)
    if not source.is_file() or source.stat().st_size > 30 * 1024 * 1024:
        raise ValueError("Source image is missing or exceeds 30 MB.")
    emit("analyzing", 3, "Preparing your source image" if request.get('background') == 'keep' else "Finding the foreground object")
    image = prepare_image(source, output.parent / "input.png", request.get('background', 'auto'))
    prepare_seconds = time.monotonic() - started
    emit("loading", 12, f"Loading TripoSR on {'Metal' if device == 'mps' else 'CPU'}")
    sys.path.insert(0, str(root / "TripoSR"))
    install_cpu_operator()
    from tsr.system import TSR
    from omegaconf import OmegaConf

    config = OmegaConf.load(root / "models" / "triposr" / "config.yaml")
    OmegaConf.resolve(config)
    model = TSR(config)
    # Only tensors are accepted; never unpickle arbitrary model code.
    checkpoint = torch.load(root / "models" / "triposr" / "model.ckpt", map_location="cpu", weights_only=True, mmap=True)
    model.load_state_dict(checkpoint)
    del checkpoint
    model.eval().to(device)
    model.renderer.set_chunk_size(4096)
    loaded_at = time.monotonic()
    emit("geometry", 25, "Reconstructing geometry from the image")
    with torch.inference_mode():
        scene_codes = model([image], device=device)
        if device == "mps":
            torch.mps.synchronize()
        inferred_at = time.monotonic()
        emit("surface", 55, "Extracting the mesh and vertex colors")
        mesh = model.extract_mesh(scene_codes, True, resolution=RESOLUTIONS[quality])[0]
    if not len(mesh.vertices) or not len(mesh.faces):
        raise ValueError("The model returned an empty mesh. Try a clearer object image.")
    if not np.isfinite(mesh.vertices).all():
        raise ValueError("The generated mesh contains invalid coordinates.")
    # Diagnostic metrics describe topology; they do not claim semantic accuracy.
    mesh_quality = {'watertight': bool(mesh.is_watertight), 'windingConsistent': bool(mesh.is_winding_consistent),
                    'components': int(len(mesh.split(only_watertight=False))),
                    'degenerateFaces': int((mesh.area_faces <= 1e-12).sum())}
    extracted_at = time.monotonic()
    emit("preparing", 92, "Writing the reconstructed GLB asset")
    # TripoSR is Z-up. glTF is Y-up. Apply a proper rotation, not a reflection.
    mesh.apply_transform(np.array([[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [0, 0, 0, 1]], dtype=float))
    mesh.metadata.update({"generator": "Sculpt / TripoSR", "simulated": False, "modelRevision": MODEL_REVISION})
    from .export import export_glb
    export_glb(mesh, output)
    metrics = {
        "engine": "triposr", "device": device, "quality": quality,
        "modelRevision": MODEL_REVISION, "sourceRevision": UPSTREAM_REVISION,
        "torchVersion": torch.__version__, "faces": len(mesh.faces), "vertices": len(mesh.vertices),
        "meshQuality": mesh_quality, "availableMemoryGbAtStart": round(available_gb, 2),
        "background": request.get('background', 'auto'),
        "materials": 1, "textureResolution": "Vertex colors", "format": "GLB",
        "prepareSeconds": round(prepare_seconds, 2),
        "loadSeconds": round(loaded_at - started - prepare_seconds, 2),
        "inferenceSeconds": round(inferred_at - loaded_at, 2),
        "surfaceSeconds": round(extracted_at - inferred_at, 2),
        "totalSeconds": round(time.monotonic() - started, 2),
        "peakProcessMemoryMb": round(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024), 1),
        "fileBytes": output.stat().st_size,
    }
    if device == "mps":
        metrics["metalAllocatedMbAtEnd"] = round(torch.mps.current_allocated_memory() / (1024 * 1024), 1)
    (output.parent / "metrics.json").write_text(json.dumps(metrics, indent=2))
    return metrics
