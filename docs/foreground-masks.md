# Foreground review and correction

In the native app, import an image and choose **Review foreground**. Sculpt runs
U2Net locally in a separate, cancellable CPU worker; it does not load TripoSR or
consume the generation trial. Paint **Add** to restore missing parts or **Erase**
to exclude background objects. Overlay, cutout, and grayscale views, brush size,
undo, and reset are available. **Use this mask** saves the selection; **Generate
3D** then reconstructs it. With Auto remove, the UI asks for mask review before
generation. Keep background remains an explicit alternative.

The original photo is EXIF-oriented and bounded to 1024 pixels on its longest
side. Preview, brush coordinates, and inference use that same coordinate system.
Rust stores immutable grayscale PNG masks by source ID and SHA-256. Job requests
retain the approved mask identity, and cached refinements retain its provenance.
Generation uses the saved mask without segmenting it again. Reopening a library
asset restores its mask. Saving an edit does not mutate an earlier job's mask.
Mask edits require a new reconstruction to change geometry; cached refinement
cannot recover image evidence excluded during the original reconstruction.

Masks are under the local library's `masks/` directory. Each generated job also
stores `source-mask.png` alongside its cropped `foreground.png` and normalized
`input.png`. These files, input photos, and model weights are never committed.

## Compositing correction

The old rembg path premultiplied RGB with the predicted alpha and then Sculpt
applied alpha again when compositing onto gray. Soft boundaries were darkened
twice. The new path retains straight RGB and applies alpha once. Existing-alpha
inputs retain their existing behavior. This is an image-preparation change, not
a more capable reconstruction model. No extraction preset, memory threshold,
model weight, or runtime dependency changed.

## Verification snapshot — 24 September 2026

- Frontend/harness tests, Python tests, native unit tests, and mask UI lifecycle
  tests cover preparation, painting, undo, saved identity, library reopening,
  and trial accounting. Browser UI tests use an explicit native IPC test double.
- A real U2Net job passed through the native Rust supervisor on the development
  Mac without creating a generation record or spending its trial.
- The macOS `.app` bundle built successfully with the mask worker included.
- All 34 CC0 evaluation photos produced masks on the M4 / 16 GB. Median worker
  time: **2.073 s**; median process wall time: **2.364 s**; maximum process RSS:
  **784.4 MB**. These sequential observations include session/model loading.
- The first reconstruction evaluation attempted all 34 photos at Draft: **29
  produced valid meshes; five were rejected by the unchanged memory guard**.
  Balanced was rejected at preflight. Results from the constrained run are not
  evidence of improved generation speed or overall reconstruction quality.
- The 33 automatically segmented model inputs differ from the old baseline
  because of the compositing correction; the existing-alpha pear is byte-identical.
  The largest channel difference is 72/255. Mask shapes are not being claimed as
  improved by this color correction.

Reproduce mask preparation and compare model inputs with a saved baseline:

```sh
OMP_NUM_THREADS=4 .sculpt-runtime/venv/bin/python backend/evaluate_masks.py \
  --output backend/outputs/YOUR_MASK_RUN \
  --baseline backend/outputs/eval-m4-draft-2026-09-17
npm run backend:eval -- run --quality draft \
  --masks backend/outputs/YOUR_MASK_RUN --output backend/outputs/YOUR_MESH_RUN
npm run backend:eval -- compare \
  --left backend/outputs/eval-m4-draft-2026-09-17 \
  --right backend/outputs/YOUR_MESH_RUN \
  --reference backend/outputs/eval-reference-2026-09-17 \
  --output backend/outputs/YOUR_COMPARISON
```

Use fresh output directories. The comparison preserves failed cases. The frozen
reference masks are automatic U2Net predictions, not independently annotated
object outlines; their silhouette IoU cannot establish better segmentation.

## Remaining evaluation work

The roadmap's mask task is **not complete**. It still needs a complete GPU run
with sufficient memory headroom, reconstruction comparisons using deliberately
corrected masks, and a fair BiRefNet/U2Net comparison with independently reviewed
reference masks. Do not change the default mask model based on agreement with
U2Net's own predictions.

BiRefNet is not installed by Sculpt or offered in the UI. An optional evaluation
model download was attempted but has not completed or been SHA-256 verified.
The official [BiRefNet code license](https://github.com/ZhengPeng7/BiRefNet/blob/main/LICENSE)
and [official weight model card](https://huggingface.co/ZhengPeng7/BiRefNet) declare
MIT. The existing locked rembg 2.0.69 adapter uses ONNX Runtime and no additional
helper model for `birefnet-general`; no new dependency or restricted helper was
added. The rembg release asset is `BiRefNet-general-epoch_244.onnx`, ID 188289669,
972,666,916 bytes, with upstream MD5 `7a35a0141cbbc80de11d9c9a28f52697`.
Complete the artifact hash/provenance check and the real memory/quality evaluation
before considering it for distribution.
