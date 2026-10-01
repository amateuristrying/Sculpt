# M4 texture-baking evaluation — 2026-10-01

34 CC0 real-photo cases; saved TripoSR Balanced meshes and scene codes from
`eval-m4-balanced-2026-09-17-complete`. No new image inference or geometry
extraction. Candidate: `texture-m4-1k-safe-2026-10-01`, 1K color texture,
10,000-face target. Hardware: Apple M4, 16 GB, Metal (PyTorch MPS).

| Measurement | Vertex-color baseline | Baked candidate |
|---|---:|---:|
| Successful results / four-view renders | 34/34 | 34/34 |
| Closed meshes before UV seam splitting | 34/34 | 34/34 |
| Median faces | 25,352 | 10,000 |
| Mean frozen-camera silhouette IoU | 0.821145 | 0.821097 |
| Mean color-field MAE (sRGB 0–255) | 6.550 | 0.536 |
| Median GLB bytes | 660962 | 1240478 |

Candidate median bake time: 2.76 s;
median isolated process wall time: 4.31 s;
maximum process RSS: 564.3 MB.
Timing was observed with other desktop activity, not a controlled speed comparison.
RSS does not measure total system or GPU memory pressure.
19/34 reached the face target. The rest retained more faces (or the original
mesh) when reduction would create invalid topology.

An initial aggressive simplification run made 27/34 closed meshes non-manifold.
That implementation was rejected. The final reducer tries gentler settings and
preserves the original if no valid reduced mesh is found. The numbers below are
from the corrected implementation, not that rejected run.

Color MAE uses 8,192 area-weighted samples on each representation's own surface
against the cached neural color field. It measures color preservation, not
photo likeness or semantic accuracy. Silhouette masks and cameras are frozen
from the baseline and are not human ground truth. Textures are sharper and
meshes often smaller in face count, but the PNG increases median file size.
TripoSR's geometry defects remain; this is not evidence of Meshy-level output.

A separate full production-worker M4 test rebuilt the shoe-shop cache at 128
and baked 2K/10,000 faces: 7.89 s total, 477 MB process RSS, closed surface,
10,000 faces. The resulting GLB passed native Rust validation. This is one 2K
smoke case, not a 34-case 2K benchmark. Blender import remains unverified.

## Per-case measurements

| Case | Faces before → after | Bake s | Peak RSS MB | Δ IoU | Color MAE before → after |
|---|---:|---:|---:|---:|---:|
| shoe-leather | 25456 → 22220 | 5.94 | 501.6 | 0.000000 | 6.508 → 0.484 |
| shoes-carpet | 23832 → 21218 | 3.32 | 535.4 | 0.000000 | 11.620 → 0.848 |
| shoe-shop | 16880 → 10000 | 1.90 | 516.3 | 0.000049 | 7.747 → 0.424 |
| shoe-repaired | 25248 → 10000 | 2.09 | 521.0 | -0.000011 | 7.104 → 0.472 |
| mug-floral | 40820 → 35186 | 8.02 | 489.8 | 0.000032 | 4.585 → 0.631 |
| mug-plastic | 33994 → 33994 | 7.17 | 485.4 | 0.000000 | 3.038 → 0.386 |
| mug-portrait | 37052 → 10000 | 2.07 | 513.8 | 0.000034 | 3.205 → 0.296 |
| mug-silver | 30680 → 10000 | 1.84 | 524.8 | -0.000448 | 6.254 → 0.425 |
| toy-jeep | 28110 → 10000 | 1.96 | 523.3 | -0.000268 | 3.604 → 0.284 |
| toy-tank | 18764 → 15868 | 3.20 | 523.3 | 0.000000 | 7.330 → 0.435 |
| toy-grass-head | 41924 → 37386 | 5.12 | 554.0 | 0.000000 | 6.687 → 1.022 |
| toy-wooden-horse | 13060 → 10000 | 1.77 | 512.3 | 0.000059 | 6.903 → 0.337 |
| plant-patio | 32196 → 32196 | 6.40 | 552.2 | 0.000000 | 8.130 → 1.075 |
| plant-stevia | 42252 → 42252 | 6.77 | 477.3 | 0.000000 | 5.838 → 0.851 |
| plant-palm | 15746 → 14220 | 2.18 | 519.0 | 0.000068 | 6.183 → 0.418 |
| plant-gerbera | 28128 → 25002 | 2.89 | 540.5 | 0.000038 | 5.884 → 0.567 |
| tool-mallet | 8044 → 8044 | 1.58 | 505.1 | 0.000000 | 5.519 → 0.247 |
| tool-bolt-cutter | 5004 → 5004 | 1.48 | 504.0 | 0.000000 | 8.190 → 0.273 |
| tool-cleco | 5600 → 5600 | 1.44 | 502.0 | 0.000000 | 8.642 → 0.278 |
| tool-sockets | 12300 → 10000 | 1.49 | 511.2 | -0.000267 | 7.844 → 0.321 |
| chair-dark-wood | 12094 → 10000 | 1.94 | 443.2 | 0.000090 | 7.076 → 0.341 |
| chair-pakri | 18492 → 10000 | 2.65 | 422.6 | 0.000113 | 6.258 → 0.363 |
| chair-upholstered | 24804 → 10000 | 2.17 | 521.5 | -0.000581 | 10.391 → 0.613 |
| chair-estonian | 14168 → 10000 | 2.16 | 517.1 | 0.000172 | 9.115 → 0.399 |
| food-blueberries | 58560 → 10000 | 2.86 | 440.3 | -0.000274 | 6.958 → 0.588 |
| food-pear | 43886 → 37866 | 10.24 | 446.2 | 0.000000 | 4.695 → 0.687 |
| food-puriri | 55432 → 55432 | 9.32 | 515.0 | 0.000000 | 6.854 → 1.728 |
| food-pineapple | 29966 → 27322 | 8.39 | 539.8 | 0.000000 | 11.518 → 1.279 |
| pot-copper | 44784 → 38220 | 11.49 | 564.3 | 0.000000 | 3.025 → 0.458 |
| bottle-calabash | 28448 → 10000 | 2.38 | 524.2 | -0.000167 | 2.959 → 0.230 |
| bottle-table | 17982 → 10000 | 2.48 | 519.1 | -0.000102 | 3.668 → 0.237 |
| bottle-yakult | 26360 → 10000 | 3.22 | 523.8 | -0.000181 | 4.058 → 0.295 |
| clutter-tools | 9664 → 9664 | 3.14 | 508.3 | 0.000000 | 8.960 → 0.387 |
| clutter-workbench | 21100 → 18966 | 3.78 | 529.9 | 0.000000 | 6.349 → 0.533 |

## Evidence and reproduction

See [the reproducible command and methods](texture-baking.md). The full offline
four-angle comparison is at
`backend/outputs/texture-m4-1k-safe-2026-10-01/comparison/index.html`.
Photos, meshes and HTML remain ignored. Dataset SHA-256:
`4c715370a3587525d810fa979fa1d4f7363ce411bd041a1891f75fc906e17580`.

Saved output SHA-256 checksums:

- `report.json`: `2257466e4e00c6e2940c90d0f59a713d54c247a7914ea40ecb61dbdbdcd9ffd5`
- `comparison/comparison.json`: `545226113c10ef1f0cec5e15708e8d6f30a37ce3daf3bea08ea2f61c8b2fce51`
- `color-fidelity.json`: `bcea6e573de4212bb61c2cd04b900f230a29e44920dc14606b76601c077cbdab`
