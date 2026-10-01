# Local color texture baking

Refine can create a textured GLB from a saved Sculpt reconstruction. It loads
only the cached scene's small decoder, without running image inference again.
Vertex colors remain the default. Selecting a texture initially keeps the
original geometry; face reduction is an explicit choice.

For 1K or 2K output:

1. Optionally reduce faces with fast-simplification 0.2.0. Try progressively
   gentler settings; reject reductions that turn a closed input into an open or
   non-manifold mesh, introduce degenerate triangles or introduce inconsistent winding. Keep
   the original when needed. The target is approximate, not guaranteed.
2. Pack UV islands using xatlas 0.0.11, retaining smooth normals at UV seams.
3. Sculpt's CPU rasterizer maps covered texture pixels to 3D surface positions.
   Query TripoSR's cached color field in batches of at most 8,192 positions.
4. Pad island gutters and embed the sRGB PNG in a nonmetallic GLB material.
   Rust validates the PNG, texture references and matching finite UV data before
   the frontend loads it. Export preserves the saved GLB bytes.

The engine supplies only a `query_colors(positions)` callback returning
normalized sRGB. The baker itself has no model dependency. The previous
vertex-color interpolation and quadratic nearest-vertex allocation are removed.
No OpenGL, nvdiffrast, cloud service, additional model weights, or new background
removal engine is involved. See the [dependency audit](texture-license-audit.md)
and bundled `backend/TEXTURE-NOTICES.txt`.

See the [34-case M4 measurements](texture-evaluation-2026-10-01.md) for before/after
results, the rejected topology regression, and limitations.

## Reproduce the evaluation

With the pinned runtime, existing 34-photo baseline and frozen reference:

```sh
npm run backend:benchmark-texture -- \
  --baseline backend/outputs/eval-m4-balanced-2026-09-17-complete \
  --output backend/outputs/texture-comparison-new \
  --reference backend/outputs/eval-reference-2026-09-17 \
  --device mps --resolution 1024 --target-faces 10000
```

This reads each saved mesh and scene, without changing extraction settings or
rerunning image inference. Each bake uses an isolated process. It writes per-case
metrics, color-field fidelity scores, and `comparison/index.html` with four
angles side by side. Photos, cached scenes, GLBs and the image-containing HTML
stay ignored. `--device cpu` must be explicit when Metal is unavailable.

The comparison uses frozen approximate photo cameras and baseline foreground
masks. IoU measures silhouettes, not semantic or 3D correctness. Color MAE uses
8,192 area-weighted samples per surface compared to the same cached neural
field; it measures storage/interpolation fidelity, not similarity to the photo.
The two meshes have different topology, so each surface is sampled separately.
Do not compare the baseline's whole image-generation time to baking-only time
as an inference speedup.

## Limits

TripoSR still determines the geometry and inferred appearance of unseen
surfaces. Sharper stored color does not fix lumpy meshes, missing thin parts or
incorrect object reconstruction. Face reduction can remove detail, and texture
PNGs may make files larger despite fewer faces. UV seam vertices are duplicated
in GLB; closed-surface diagnostics are measured before UV splitting.

Only the M4/16 GB runtime is validated. Browser mode remains a clearly labeled
demo, not image reconstruction. Blender import has not yet been checked on this
machine. Baked color is not generated PBR roughness/metalness/normal maps.
