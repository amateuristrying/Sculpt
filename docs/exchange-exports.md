# Saved-asset export

Export now offers GLB, embedded glTF, OBJ and binary STL. These are native
saved-asset operations; no model load, Python process, or frontend mesh upload
is needed. Browser demo export remains GLB only.

| Format | What is saved | Limits |
| --- | --- | --- |
| GLB | Original verified bytes, geometry and material/texture | Most faithful round trip |
| glTF | The same document and binary data, with the buffer embedded as a data URI | Base64 makes the file larger; no sidecar files |
| OBJ | ZIP of `model.obj`, `model.mtl`, and referenced PNGs | Unzip before import; basic diffuse material, not full PBR |
| STL | Binary triangles scaled to the requested height in mm | Geometry only; see [STL notes](stl-export.md) |

For textured assets, choose **Refine → Texture output → 1K or 2K → Apply
refinement**, then export the result. Export retains the saved texture size.
It does not claim that upscaling a texture adds reconstructed detail. Image
size, mesh complexity and compression determine file size; a 66 MB GLB is not
necessarily better geometry than a 2 MB GLB.

OBJ keeps the original Y-up coordinates, applies scene transforms and handles
instances, nonuniform scales, reflected winding and inverse-transpose normals.
UVs are converted to Wavefront's image convention; PNG bytes are copied without
resampling. Global vertex/normal/UV offsets support multiple primitives.
Linear vertex colors are converted to sRGB using the common OBJ vertex-color
extension. Many importers ignore that extension, so bake a texture for portable
color. Full metallic/roughness/alpha semantics are not preserved by the basic
MTL export; choose GLB/glTF for those. Animated/skinned meshes, morph targets,
required glTF extensions and texture transforms are rejected by OBJ conversion.
Current Sculpt output is static, opaque and uses a diffuse color texture.

Rust validates the saved asset, converts off the UI thread and outside the
library lock, then opens the native save dialog. The frontend sends only the
asset ID, format and filename. Fixed archive filenames avoid cross-file naming
errors; package size is bounded to 256 MB (GLB input remains bounded to 128 MB).
JSON uses serde_json's float-roundtrip option to preserve metadata precision.

## Verification on M4 / 16 GB, 2026-10-15

The existing CC0 `toy-wooden-horse` evaluation photo's saved Draft reconstruction
was exported, followed by a cached 1K texture bake with unchanged extraction
settings and no decimation. A separate authored asymmetric color fixture checks
UV orientation. This is conversion verification, not a new 34-photo model-quality
benchmark: no model, extraction or bake algorithm was changed.

| Saved asset | GLB bytes | glTF bytes | OBJ ZIP bytes | Faces before → after | Max triangle-position error |
| --- | ---: | ---: | ---: | ---: | ---: |
| Vertex colors | 412,232 | 549,131 | 1,987,749 | 15,792 → 15,792 | 0 |
| 1K baked color | 1,382,748 | 1,843,117 | 3,569,765 | 15,792 → 15,792 | 0 |

Independent trimesh re-import preserved triangle winding and watertightness
(after welding UV seams), matched texture UVs, and recovered identical PNG
pixels. The embedded glTF retained an identical binary payload and equal JSON
values. This does **not** constitute Blender, Quick Look or slicer validation.
USDZ remains unimplemented; adding it requires a converter and Apple viewer QA.

The unchanged 1K bake on this saved scene took 7.74 s total, including 2.59 s
decoder load and 2.21 s baking, with 485.0 MB peak process memory and zero image
inference. This single smoke run is not a general memory budget or speed claim.
The original source SHA-256 is recorded in `backend/evaluation/photos.json`.
No photos, model caches or exported assets are committed.

Reproduce with any saved GLB:

```sh
SCULPT_EXCHANGE_INPUT=/absolute/path/mesh.glb \
SCULPT_EXCHANGE_OUTPUT=/absolute/path/export-check \
  cargo test --manifest-path src-tauri/Cargo.toml --locked \
  export_saved_exchange_assets -- --ignored --nocapture
.sculpt-runtime/venv/bin/python backend/verify_exchange_exports.py /absolute/path/export-check
```

## Dependency audit

The only added compiled library is [zip 4.6.1](https://docs.rs/crate/zip/4.6.1),
MIT, pinned with default features disabled. Archives use the stored method:
no compression, encryption, OpenGL or native codec dependency. The enabled
dependency tree reuses crc32fast 1.5.2, cfg-if 1.0.4, indexmap 2.14.2,
equivalent 1.0.2, hashbrown 0.17.1 and memchr 2.8.3. Their installed crate
manifests and license files were inspected; all offer MIT. Notices are bundled
in `EXPORT-NOTICES.txt` and checked by the app bundle verifier.
Cargo also records zip's fuzzing-only arbitrary/derive_arbitrary dependencies;
both 1.5.0 manifests declare MIT OR Apache-2.0 (checked from crates.io archives).
They are not built or distributed in Sculpt's normal build.
No model weights, helper models or inference dependencies were added.
