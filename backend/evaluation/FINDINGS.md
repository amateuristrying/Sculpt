# M4 baseline — 17 September 2026 (UTC)

Task 1 ran all 34 independently sourced CC0 photographs through the existing
TripoSR worker on the actual **Apple M4 / 16 GB** development Mac, once in Draft
and once in Balanced. No inference code, extraction preset, mask model, cleanup
default, or runtime dependency changed. The [recorded numerical baseline](baseline-m4-2026-09-17.json)
includes source/artifact hashes, frozen camera estimates, per-case metrics, and
model/runtime pins. Photos, meshes, masks, and HTML remain local and ignored.

All 34 pairs have byte-identical prepared inputs and foreground masks, and exactly
equal cached neural scene codes (maximum float difference 0). The compared geometry
therefore differs at extraction resolution, not at image preprocessing or inference.

| Measurement | Draft (96) | Balanced (128) |
| --- | ---: | ---: |
| Valid GLBs / attempted | 34 / 34 | 34 / 34 |
| Median worker time | 9.90 s | 11.20 s |
| Median end-to-end CLI time | 11.01 s | 12.43 s |
| Observed worker-time range | 8.46–24.09 s | 8.43–17.61 s |
| Maximum process RSS | 4,010.0 MB | 4,022.6 MB |
| Lowest available memory **at job start** | 2.96 GB | 5.08 GB |
| Median faces | 14,020 | 25,352 |
| Watertight meshes | 34 / 34 | 34 / 34 |
| Meshes with disconnected components | 24 / 34 | 24 / 34 |
| Mean frozen-reference silhouette IoU | 0.8263 | 0.8211 |

These are sequential observations on a shared desktop, not controlled timing
trials. Draft ran first. Background load, memory reclamation, warm caches, and
thermal state were not controlled. The first Balanced attempt was rejected by
the existing 3 GB memory guard; the completed Balanced run started after headroom
recovered. That failed attempt remains in local outputs and is not mixed into the
successful reconstruction baseline. No memory guard was relaxed.

RSS does not measure peak GPU allocation or total unified-memory pressure. The
start-of-job memory observation is not minimum headroom during inference. These
numbers do not justify raising presets or declaring larger models safe on 16 GB.

## Interpretation

Balanced creates about **81% more faces** while this outline metric changes by
**−0.0051 IoU**. Higher extraction resolution alone is not a reliable way to improve
the input outline. This does **not** prove Draft has better overall visual quality:
the score ignores color detail and hidden surfaces, can favor thick/blobby geometry,
and uses an estimated camera. The reference masks are 33 automatic U2Net outputs
plus one existing source alpha, not human annotations. The camera was fitted on
Balanced and then held fixed for both runs.

Visual inspection of the source, prepared input, and front/reverse renders found:

| Cases | Visible finding | Next useful experiment |
| --- | --- | --- |
| `shoe-shop`, `shoe-repaired` | Shoe outlines are recognizable; surface and hidden sides remain rough. | Cleanup and texture work after a trustworthy mask. |
| `mug-floral`, `mug-silver`, `pot-copper` | Main body and handle are present; thin handle detail and inferred backs remain uneven. | Compare cleanup without destroying handles. |
| `tool-mallet`, `tool-bolt-cutter` | The held hand survives background removal and becomes geometry. | Let the user erase the hand before inference. |
| `chair-upholstered` | The prepared mask removes the legs, so the reconstruction contains mainly seat/back. | Repair the mask; more extraction samples cannot recover removed image evidence. |
| `plant-palm` | Masking loses fine fronds and the generated foliage becomes lumpy. | Independently reviewed masks; measure thin-structure retention. |
| `toy-wooden-horse` | Multiple displayed objects and part of the support remain, producing separate pieces. | Explicit foreground selection before generation. |
| `clutter-tools` | Masking selects mainly the spirit levels, discarding most of the scene. | Show and correct the intended object selection. |
| `clutter-workbench` | Retained clutter becomes an irregular combined surface. | Single-object selection; do not present scene reconstruction as supported. |
| Transparent bottles | Bottle silhouettes are recognizable but glass is reconstructed as opaque colored surfaces. | Keep transparency/material limitations explicit. |

Watertightness is an engineering property, not a likeness, manifold printability,
or useful-asset guarantee. Separate components can each be watertight. The report
shows all failures and awkward inputs; none were removed to improve the headline.

## Local artifacts and reproduction

The development checkout contains:

```text
backend/outputs/eval-m4-draft-2026-09-17/
backend/outputs/eval-m4-balanced-2026-09-17-complete/
backend/outputs/eval-reference-2026-09-17/
backend/outputs/eval-comparison-2026-09-17/index.html
```

The comparison rendered **272 views** (34 cases × 2 runs × 4 angles), with all 34
pairs scored. To recreate a page from these saved local runs, choose a fresh output
directory:

```sh
npm run backend:eval -- compare --left backend/outputs/eval-m4-draft-2026-09-17 --right backend/outputs/eval-m4-balanced-2026-09-17-complete --reference backend/outputs/eval-reference-2026-09-17 --output backend/outputs/my-comparison
```

On another checkout, follow the [fetch/run/reference/compare instructions](README.md).
The numerical baseline is committed, but generated meshes and source photos are
not. New runs can vary; do not substitute a fresh mask or newly fitted camera when
claiming a regression comparison against an existing reference.

The next task is mask preview/correction and a measured remover comparison. Before
comparing U2Net with BiRefNet, create independently reviewed reference masks; scoring
BiRefNet against U2Net's own output would bias that decision. No default model or
quality setting changes are justified by this baseline alone.
