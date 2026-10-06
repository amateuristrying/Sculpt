# Native STL export

Sculpt can export a completed desktop reconstruction as a binary STL from its
saved asset ID. The browser demo never sends a mesh to this command. Rust reads
the already validated GLB from the local library, so the UI does not copy a
large mesh through JSON and a cancelled save cannot mutate the library.

The exporter walks the selected glTF scene, applies node matrix or TRS
transforms, rejects skinned or morph-target meshes, and writes each triangle as
the 50-byte little-endian binary STL record. STL has no unit or material
metadata; Sculpt therefore asks for an explicit object height in millimetres,
uses a uniform scale, converts the glTF Y-up coordinates to Z-up, centers X/Y,
and places the lowest point at Z=0. Reflected transforms reverse triangle
winding so the exported normals keep the source orientation.

The output is deliberately geometry-preserving. It does not weld vertices,
fill holes, discard components, or claim that a TripoSR result is printable.
The export panel calls out that open or disconnected surfaces should be checked
in a slicer. OBJ, PLY and 3MF are separate planned formats because they have
different material, unit, and metadata requirements.

## Verification

The native unit tests cover indexed and non-indexed meshes, nested transforms,
column-major matrices, reflected winding, invalid sizes, and unsupported node
features. The ignored batch test exports the current 34-case M4 evaluation run:

```sh
SCULPT_STL_INPUT="$PWD/backend/outputs/texture-m4-1k-safe-2026-10-01" \
SCULPT_STL_OUTPUT="$PWD/backend/outputs/stl-eval-2026-10-02" \
cargo test --manifest-path src-tauri/Cargo.toml export_evaluation_run -- \
  --ignored --nocapture
```

The Python verifier then loads both GLB and STL with trimesh and compares face
counts, transformed positions, winding consistency, volume, component count,
closed-surface state, and the requested height:

```sh
.sculpt-runtime/venv/bin/python backend/verify_stl_exports.py \
  backend/outputs/texture-m4-1k-safe-2026-10-01 \
  backend/outputs/stl-eval-2026-10-02
```

The current run passes all 34 saved meshes at 100 mm. This validates the native
conversion and scaling invariants; it is not a claim that every reconstructed
mesh is suitable for printing. Blender and a slicer are not installed on the
development Mac, so those external application checks remain release work.
