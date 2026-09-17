import json
import hashlib
import os
from pathlib import Path
import resource
import sys
import time

from .config import MODEL_REVISION, UPSTREAM_REVISION, SPEC

RESOLUTIONS = {profile['id']: profile['resolution'] for profile in SPEC['qualities']}


def extraction_chunk_size(value=None):
    """Bounded developer benchmark override; native UI never supplies this field."""
    if value is None:
        return 4096
    if type(value) is not int or value not in {4096, 8192, 16384}:
        raise ValueError('Query chunk size must be 4096, 8192, or 16384.')
    return value


def generate(request: dict, root: Path, emit) -> dict:
    import numpy as np
    import torch

    from .images import prepare_image
    from .marching import install_cpu_operator
    from .memory import check_memory
    from .refinement import clean_mesh, refinement_settings, check_refinement_memory
    from .scene_cache import CACHE_FILENAME, read_scene_cache, write_scene_cache, validate_source_hash

    started = time.monotonic()
    operation = request.get('operation', 'generate')
    if operation not in {'generate', 'refine'}:
        raise ValueError('Unknown reconstruction operation.')
    quality = request.get('quality', 'draft')
    if quality not in RESOLUTIONS:
        raise ValueError('Unknown geometry quality.')
    settings = refinement_settings(request.get('refinement'), RESOLUTIONS[quality])
    chunk_size = extraction_chunk_size(request.get('queryChunkSize'))
    output = Path(request['outputPath'])
    cached_scene, cache_metadata = None, None
    if operation == 'refine':
        cache_path = Path(request['sceneCachePath'])
        if output.parent.resolve() == cache_path.parent.resolve():
            raise ValueError('Refinement must create a new asset, preserving the original.')
        source_sha = validate_source_hash(request.get('sourceSha256'))
        cached_scene, cache_metadata = read_scene_cache(cache_path, source_sha)
    else:
        source = Path(request['sourcePath'])
        if not source.is_file() or source.stat().st_size > 30 * 1024 * 1024:
            raise ValueError('Source image is missing or exceeds 30 MB.')
        source_sha = hashlib.sha256(source.read_bytes()).hexdigest()
        if request.get('sourceSha256') is not None and request['sourceSha256'] != source_sha:
            raise ValueError('The source image changed after import. Import it again.')
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
    output.parent.mkdir(parents=True, exist_ok=True)
    import psutil
    available_gb = psutil.virtual_memory().available / (1024**3)
    # An explicit refinement resolution cannot bypass a higher quality's guard.
    memory_quality = 'high' if settings['resolution'] > 128 else 'balanced' if settings['resolution'] > 96 else 'draft'
    if operation == 'refine':
        check_refinement_memory(settings['resolution'], available_gb)
    else:
        check_memory(memory_quality, available_gb)
    background = cache_metadata['background'] if cache_metadata else request.get('background', 'auto')
    if operation == 'generate':
        emit('analyzing', 3, 'Preparing your source image' if background == 'keep' else 'Finding the foreground object')
        image = prepare_image(source, output.parent / 'input.png', background)
    else:
        emit('analyzing', 3, 'Restoring the saved scene; image reconstruction will be reused')
    prepare_seconds = time.monotonic() - started
    emit('loading', 12, f"Loading TripoSR {'surface decoder' if operation == 'refine' else 'model'} on {'Metal' if device == 'mps' else 'CPU'}")
    sys.path.insert(0, str(root / "TripoSR"))
    install_cpu_operator()
    from tsr.system import TSR
    from omegaconf import OmegaConf

    config = OmegaConf.load(root / "models" / "triposr" / "config.yaml")
    OmegaConf.resolve(config)
    if operation == 'refine':
        from tsr.utils import find_class

        class SceneDecoder(TSR):
            # Retain pinned upstream extraction behavior while loading only the
            # modules needed to query a saved scene. No image encoder or transformer.
            def configure(self):
                self.decoder = find_class(self.cfg.decoder_cls)(self.cfg.decoder)
                self.renderer = find_class(self.cfg.renderer_cls)(self.cfg.renderer)
                self.isosurface_helper = None

        model = SceneDecoder(config)
    else:
        model = TSR(config)
    # Only tensors are accepted; never unpickle arbitrary model code.
    checkpoint = torch.load(root / "models" / "triposr" / "model.ckpt", map_location="cpu", weights_only=True, mmap=True)
    if operation == 'refine':
        checkpoint = {key: value for key, value in checkpoint.items() if key.startswith(('decoder.', 'renderer.'))}
    model.load_state_dict(checkpoint)
    del checkpoint
    model.eval().to(device)
    model.renderer.set_chunk_size(chunk_size)
    loaded_at = time.monotonic()
    emit('geometry', 25, 'Reconstructing geometry from the image' if operation == 'generate' else 'Reusing the saved scene representation')
    with torch.inference_mode():
        scene_codes = model([image], device=device) if operation == 'generate' else torch.from_numpy(cached_scene).to(device)
        if device == "mps":
            torch.mps.synchronize()
        inferred_at = time.monotonic()
        cache_output = output.parent / CACHE_FILENAME
        write_scene_cache(cache_output, scene_codes.detach().cpu().numpy().astype(np.float32), source_sha, background)
        emit("surface", 55, "Extracting the mesh and vertex colors")
        mesh = model.extract_mesh(scene_codes, True, resolution=settings['resolution'], threshold=settings['densityThreshold'])[0]
        before_cleanup = {'faces': len(mesh.faces), 'vertices': len(mesh.vertices)}
        mesh, cleanup_metrics = clean_mesh(mesh, settings)
        if settings['smoothingIterations']:
            # Re-query color at moved vertices so smoothing does not drag old
            # samples over the surface. mesh remains in TripoSR's Z-up coordinates.
            positions = torch.as_tensor(np.asarray(mesh.vertices), dtype=scene_codes.dtype, device=device)
            mesh.visual.vertex_colors = model.renderer.query_triplane(model.decoder, positions, scene_codes[0])['color'].cpu().numpy()
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
        'operation': operation, 'refinement': settings,
        'queryChunkSize': chunk_size,
        'cleanup': {**cleanup_metrics, 'before': before_cleanup,
                    'after': {'faces': len(mesh.faces), 'vertices': len(mesh.vertices)}},
        'canRefine': True, 'sceneCacheSha256': hashlib.sha256(cache_output.read_bytes()).hexdigest(),
        'sourceSha256': source_sha,
        "modelRevision": MODEL_REVISION, "sourceRevision": UPSTREAM_REVISION,
        "torchVersion": torch.__version__, "faces": len(mesh.faces), "vertices": len(mesh.vertices),
        "meshQuality": mesh_quality, "availableMemoryGbAtStart": round(available_gb, 2),
        "background": background,
        "materials": 1, "textureResolution": "Vertex colors", "format": "GLB",
        "prepareSeconds": round(prepare_seconds, 2),
        "loadSeconds": round(loaded_at - started - prepare_seconds, 2),
        "inferenceSeconds": round(inferred_at - loaded_at, 2) if operation == 'generate' else 0,
        "surfaceSeconds": round(extracted_at - inferred_at, 2),
        "totalSeconds": round(time.monotonic() - started, 2),
        "peakProcessMemoryMb": round(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024 if sys.platform == 'darwin' else 1024), 1),
        "fileBytes": output.stat().st_size,
    }
    if device == "mps":
        metrics["metalAllocatedMbAtEnd"] = round(torch.mps.current_allocated_memory() / (1024 * 1024), 1)
    from .files import atomic_write_bytes
    atomic_write_bytes(output.parent / "metrics.json", json.dumps(metrics, indent=2).encode("utf-8"))
    return metrics
