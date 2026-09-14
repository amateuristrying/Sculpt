# Sculpt Inference Harness

The workspace imports this facade instead of a model SDK. Native builds use Tauri
commands and events; Vite previews use an explicitly labelled M4 fixture and a local
mock timer. Native detection failures are never replaced with the preview fixture.

```text
React workspace → Sculpt Harness → selected runtime adapter → AI engine → local GPU
                         │
                         └── MockRuntime (the only implemented adapter today)
```

`src-tauri/src/harness` owns detection, compatibility, job lifetime, cancellation,
and runtime selection. `InferenceRuntime` is the replaceable Rust adapter boundary.
An adapter may supervise a Python/MLX/PyTorch worker process. ONNX, WebGPU, and native
runtimes are separate options; models do not need to share one execution format.

Engine recommendations are prototype suitability targets. All catalog entries have
`implemented: false`, and their descriptions explain missing adapters and estimated
model sizes. Unsupported profiles are rejected again in Rust using actual detected
hardware, even if a caller supplied a different profile to the recommendation query.

## Image input boundary

The current generation request passes `imageName` only to identify the procedural
fixture. It performs no image understanding. Source image pixels remain in the UI
for preview. Before adding real inference, introduce native image import that checks
file type/size, decodes the image, and returns a local source asset ID. Pass that ID
through `GenerationRequest`; let the harness resolve it for the selected runtime.
Do not couple workspace components to filesystem paths or any engine's input schema.

## Next adapter integration points

- Extend hardware capability checks and validate an engine's actual memory budget.
- Add native model download, checksum, loading, and runtime health logic using the
  `ModelArtifact`, `ModelState`, and runtime descriptors as the public vocabulary.
- Replace explicit `MockRuntime` routing only when a validated adapter is installed.
- Return a generated asset reference with geometry/material metadata, preserving
  existing progress events and cooperative cancellation.
- Add explicit fallback policy in the harness; never silently claim another engine
  generated the requested result.

AI inference, image editing, model installation, accounts, and network inference are
not implemented by this prototype. GLB export is real: the viewport serializes the
asset, then Rust validates its binary header and writes only to the path chosen in
the native save dialog.
