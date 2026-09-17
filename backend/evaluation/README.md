# Real-photo evaluation

This is the quality regression set, separate from the older TripoSR showcase
smoke tests. `photos.json` records 34 independently sourced photographs of shoes,
mugs, toys, plants, tools, furniture, food, reflective metal, transparent glass,
and clutter. It includes amateur indoor/outdoor photos, museum object photography,
hand-held objects, busy backgrounds, pairs, thin structures, and cropped subjects.
No photo comes from TripoSR's examples. It is a small diagnostic set, not a random
sample of customer inputs or proof of commercial-generator parity.

The [M4 baseline findings](FINDINGS.md) record 68 actual Metal reconstructions,
four-view comparisons, numerical results, and visible failure cases.

## Data provenance and licenses

Every selected photograph is marked **CC0** on its Wikimedia Commons file page.
The manifest records the author, source page, original and downloaded image URLs,
CC0 license URL, retrieved license metadata, dimensions, byte count, and SHA-256
of the exact downloaded bytes. Selection included visual inspection to exclude
drawings, renders, duplicate views of the same object, and unrelated search hits.
Some cases show multiple objects intentionally; that is not a promise that the
single-object model can handle them.

The [CC0 dedication](https://creativecommons.org/publicdomain/zero/1.0/) covers
copyright reuse; it does not imply endorsement by the photographers or museums.
Per-photo license evidence is in `photos.json`. Source files are originals or
published previews (exact dimensions and URLs are recorded). Four smaller sources
use 500-pixel-wide previews; the others are originals or 1280-pixel-wide previews.
Do not replace a photo, re-encode it, or silently refresh its hash: that creates a
new dataset version and invalidates comparisons with this baseline.

**Photos, masks, meshes, cached scenes, and HTML reports stay under the ignored
`backend/outputs/` directory. They are never committed.** Only provenance, code,
and numerical measurements belong in Git. No new dependency or model is added:
the renderer uses the already installed NumPy (BSD), Pillow (MIT-CMU), and trimesh
(MIT). Inference and background removal use the existing pinned runtime.

## Reproduce

Install the local runtime first (`npm run backend:setup`, or Sculpt's setup screen).
Only fetching needs network access. Reconstruction and comparisons run offline.

```sh
# Download only missing files; verify every existing/downloaded SHA-256.
npm run backend:eval -- fetch

# One isolated worker at a time. Existing output directories are never overwritten.
npm run backend:eval -- run --output backend/outputs/eval-baseline --quality balanced

# Freeze the baseline's automatic masks and estimated input-view cameras once.
npm run backend:eval -- reference --run backend/outputs/eval-baseline --output backend/outputs/eval-reference

# Make a second run after an intentional change (or repeat to measure variability).
npm run backend:eval -- run --output backend/outputs/eval-candidate --quality balanced

# ONE comparison command: four angles per result + scores + standalone HTML.
npm run backend:eval -- compare --left backend/outputs/eval-baseline --right backend/outputs/eval-candidate --reference backend/outputs/eval-reference --output backend/outputs/eval-comparison
open backend/outputs/eval-comparison/index.html
```

The comparison accepts any two evaluation runs using the same dataset and frozen
reference; they do not have to include the baseline itself. Comparing a run with
itself is supported as a zero-delta check. The HTML embeds its images and needs no
server or CDN. `comparison.json` contains per-case and per-category scores, paired
deltas, failure counts, median runtime, and maximum process RSS. A failed or
missing case stays visible. Paired means use only cases scored on both sides;
the separate `meanIouFailuresAsZero` retains the whole-set denominator. The run
records hardware, code commit/dirty state, pinned runtime spec, source hashes,
timings, topology, and worker errors. A failed reconstruction makes `run` return 1
but still saves the report and continues through the rest of the set.

Wikimedia may rate-limit downloads. Fetch respects `Retry-After`, reuses verified
downloads, and rejects changed bytes. If a source disappears, report it and review
a replacement as a new dataset version. Do not lower the provenance requirement.

## What the score means

IoU = foreground intersection / foreground union, measured at 256 × 256 pixels.
The reference is the baseline worker's **foreground alpha mask, not a human
segmentation**. It uses automatic U2Net for 33 photos and existing source alpha
for `food-pear`, matching the app's preprocessing. Alpha is thresholded at >32, matching the foreground bounding-box
rule, and nearest-neighbor resized after the same crop/centering as model input.
The page shows both the original photo and normalized input. The overlap panel
uses white for agreement, blue for mask-only pixels, and orange for mesh-only pixels.

TripoSR does not return physical camera calibration. Reference creation estimates
the photo view **once from the baseline**, using a coarse silhouette search around
the origin: azimuth in 45° steps, elevation −15/0/15/30°, distance 1.6/1.9/2.2/2.5,
then ±15° azimuth adjustment, at fixed 40° FOV. This is an approximate alignment,
not measured camera intrinsics or ground-truth pose. Symmetric outlines can have
ambiguous angles. Inspect the reference view before using a case to make decisions.
The baseline has an alignment advantage; do not interpret the score as an absolute
quality ranking across unrelated models.

All later comparisons reuse those exact cameras and masks. They never fit, center,
resize, or mirror candidate meshes. Four views use the estimated photo angle and
90/180/270° offsets at the same elevation. The CPU renderer applies GLB scene
transforms, perspective projection, a triangle depth buffer, perspective-correct
vertex colors, and soft two-sided lighting. Final masks use triangle coverage at
pixel centers; the faster low-resolution polygon union is only used for camera
search. Texture maps are not yet rendered by this evaluation tool.

**A wrong mask can reward a wrong reconstruction.** IoU cannot judge hidden surfaces,
texture detail, correct object identity, or printability. Mask comparison in task 2
will need independently reviewed reference masks, rather than treating U2Net as
ground truth. Transparent objects, reflections, hands, and clutter are intentional
failure probes. Always inspect the photo and all views alongside the numbers.

## Hardware and interpretation

Runs currently use TripoSR on **MPS**, the only validated inference backend.
The baseline target is the M4 MacBook Air with 16 GB unified memory. No CUDA,
Windows, or Linux hardware is claimed validated by these tools. Measurements are
sequential single-process observations; filesystem caches, other applications,
temperature, and power state affect timing. Peak process RSS is **not total unified
memory pressure or peak GPU allocation**. Available memory at job start is not the
minimum headroom during execution. A future preset change must measure that too.

Generation presets remain Draft 96 / Balanced 128 / High 192 with no cleanup
enabled by this evaluation change. It establishes a baseline; it does not change
Sculpt's generated output.
