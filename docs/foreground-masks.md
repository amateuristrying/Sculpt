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

## Reconstruction comparison

The [recorded per-case measurements](../backend/evaluation/mask-review-m4-2026-09-23.json)
retain all 34 attempted cases, including the five memory rejections. For the 29
successful pairs at the same Draft resolution, frozen-reference silhouette IoU
changed from **0.8203 to 0.8222** (delta **+0.0019**). This small mean conceals mixed
results: `tool-mallet` changed by −0.2149 and `bottle-table` by −0.1352, while
`toy-wooden-horse` changed by +0.1968 and `plant-patio` by +0.1236.

Visual review of front/reverse renders shows the main remaining problems in both
versions: the mallet's hand becomes geometry, clutter joins the toy, and hidden
surfaces remain rough. The changed bottle scale/outline also illustrates the
sensitivity of IoU to a fixed estimated camera. **The measurements do not establish
an overall quality improvement.** The compositing correction is kept explicit;
manual foreground selection and stronger reconstruction still need evaluation.

Observed median worker times across each run's successful cases were 9.90 s before
and 17.78 s after. Maximum process RSS was 4,010.0 MB and 3,538.7 MB respectively.
The run conditions and successful subsets differ; neither number is a controlled
performance comparison. Source-mask preparation is a separate operation and must
be included when measuring the user's full first-generation workflow.

## U2Net / BiRefNet CPU comparison — 25 September 2026

Both models completed all 34 photos (33 segmentations and one existing-alpha
passthrough) in fresh CPU workers, four threads per worker. The
[per-case record](../backend/evaluation/mask-models-cpu-2026-09-25.json) includes
source/mask hashes, model checksums, observations and failures.

| Observation | U2Net | BiRefNet general |
| --- | ---: | ---: |
| Successful cases | 34 / 34 | 34 / 34 |
| Median worker time, including loading | 1.197 s | 19.350 s |
| Median process wall time | 1.317 s | 19.841 s |
| Maximum process RSS | 872.2 MB | 5,883.9 MB |
| Lowest sampled available memory | 4.579 GB | 2.232 GB |

The repeated U2Net masks match the earlier selections at every thresholded pixel.
BiRefNet/U2Net mean mask agreement IoU is **0.8502**. That is agreement, **not
accuracy**. Visual inspection finds useful BiRefNet improvements: it retains the
mallet head, bolt-cutter jaws and toy-jeep arm that U2Net partially discards, and
removes more background between palm leaves. It still retains a hand, the toy
horse's shelf and second toy, and a second bottle where the intended subject is
ambiguous. Neither model selects a user's intended object reliably in clutter.

**Keep U2Net as the default.** BiRefNet's roughly 16× median worker time and higher
memory cost need an independently scored quality benefit before a default switch.
These sequential runs used the existing M4 development machine, but this execution
environment denied native chip probes; the reports preserve unknown fields and
the probe errors. RAM is reported through psutil. This is a CPU mask evaluation,
not a new GPU or platform validation. Shared desktop load and thermal state were
not controlled. Peak RSS is not total unified-memory pressure.

## Corrected selection: actual reconstruction

The wooden-horse case contains two toys and a shelf. Keeping only the upper horse
before reconstruction removes the shelf and lower toy from the generated asset.
This was tested with two real CPU TripoSR jobs, using the same model and Draft 96
settings. The [recorded selection polygon and measurements](../backend/evaluation/mask-correction-cpu-2026-09-25.json)
retain the source identity and both immutable mask identities.

| Observation | Automatic mask | Corrected mask |
| --- | ---: | ---: |
| Faces | 15,792 | 7,452 |
| Connected components | 2 | 1 |
| Watertight | Yes | Yes |
| Worker time | 8.29 s | 5.98 s |
| Maximum process RSS | 3,619.7 MB | 3,628.8 MB |

Four-angle inspection confirms the intended object is isolated; rough surface
detail and guessed hidden geometry remain. This establishes that saved mask
corrections affect real reconstruction. Fewer faces or components alone do not
establish quality, and one case is not a whole-set improvement. Changed selection
also changes the model's crop/scale, so the old full-scene silhouette is not an
appropriate scoring target. Timing is a single pair with different cache conditions.

## Remaining evaluation work

The roadmap's mask task is **not complete**. It still needs a complete GPU run
with sufficient memory headroom, more corrected-mask reconstruction cases, and
independently reviewed reference masks for a fair segmentation accuracy score.
Do not change the default based on agreement with U2Net's own predictions.

## Optional model provenance and reproduction

BiRefNet is not installed by Sculpt or offered in the UI. The optional evaluation
model is downloaded outside the installed runtime and pinned to SHA-256
`58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c`.
The official [BiRefNet code license](https://github.com/ZhengPeng7/BiRefNet/blob/main/LICENSE)
and [official weight model card](https://huggingface.co/ZhengPeng7/BiRefNet) declare
MIT. The existing locked rembg 2.0.69 adapter uses ONNX Runtime and no additional
helper model for `birefnet-general`; no new dependency or restricted helper was
added. The rembg release asset is `BiRefNet-general-epoch_244.onnx`, ID 188289669,
972,666,916 bytes, with upstream MD5 `7a35a0141cbbc80de11d9c9a28f52697`.
The artifact's size and upstream MD5 were verified before pinning its SHA-256.
`backend/sculpt_eval/mask_models.py` uses the existing locked rembg adapter and
ONNX Runtime on CPU, with its download method replaced by the verified local
path. No remote Python code, extra helper model, or package was installed.
Complete the independent quality evaluation before considering distribution.

Optional, evaluation-only comparison (download the reviewed artifact explicitly;
the evaluator never fetches weights):

```sh
.sculpt-runtime/venv/bin/python backend/evaluate_masks.py \
  --model birefnet-general \
  --weights backend/outputs/evaluation-models/birefnet-general.onnx \
  --reference-masks backend/outputs/YOUR_U2NET_MASK_RUN \
  --output backend/outputs/YOUR_BIREFNET_MASK_RUN
.sculpt-runtime/venv/bin/python backend/compare_masks.py \
  --left backend/outputs/YOUR_U2NET_MASK_RUN \
  --right backend/outputs/YOUR_BIREFNET_MASK_RUN \
  --output backend/outputs/YOUR_MASK_COMPARISON
```

`--case CASE_ID` limits a smoke test. Each case gets a fresh CPU process, a
five-minute timeout, and a sampled 1 GB available-memory floor. Failures remain in
the report. Native chip probes that are denied by the environment are recorded as
unknown, with the probe error; they never establish new hardware validation.
The HTML report is self-contained and labels automatic-mask agreement separately
from accuracy. Independent reviewed reference masks are still needed for that.
