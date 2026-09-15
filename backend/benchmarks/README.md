# Local reconstruction checks

These benchmarks test pipeline reliability and expose reconstruction failures. They
do not measure semantic accuracy, establish Meshy parity, or imply production-ready
topology. Review source images, material views, and reverse angles separately.

## Reproduce

Install the development runtime using Sculpt's setup screen under `npm run desktop`,
or `npm run backend:setup`. The starter manifest uses five images from the pinned
TripoSR source checkout and one banana photograph. Place the photograph at
`backend/outputs/banana/source.jpg` before running the full starter set. The baseline
uses [Banana-Single.jpg by Evan-Amos](https://commons.wikimedia.org/wiki/File:Banana-Single.jpg),
CC BY-SA 3.0; source hashes and credits are recorded in every report. You can instead
create a manifest referencing your own images. No photos or meshes are committed.

```sh
npm run backend:benchmark -- --manifest backend/benchmarks/starter.json --output backend/outputs/my-run --quality balanced
```

Use a fresh output directory for every run. Each case records its source SHA-256,
worker log, prepared image, GLB, timings, process memory, and topology metrics.
Validation reads the exported GLB independently, rejects external resources, and
checks finite, nonempty geometry. A passing case means the expected engineering
outcome occurred; `visualReview` intentionally stays pending until a person reviews it.

Create reproducible stress inputs without downloading additional images:

```sh
.sculpt-runtime/venv/bin/python backend/benchmarks/prepare_edge_cases.py
npm run backend:benchmark -- --manifest backend/outputs/edge-fixtures/manifest.json --output backend/outputs/my-edge-run --quality balanced
```

This derives transparent, rotated, occluded, and two-object variants from the pinned
examples. It also tests background preservation and expects an empty transparent PNG
to fail without producing a mesh. These are synthetic stress cases, not six new
real-world photographs.

To load every successful result through the UI, inspect four display modes and a
second viewing angle, and verify byte-identical export:

```sh
SCULPT_BENCHMARK_OUTPUT=backend/outputs/my-run npm run test:ui
```

Google Chrome must be installed. Screenshots are written under `test-results/`.
This browser test uses a native-IPC test double with the actual GLB files; it does
not replace the Rust GPU integration test or native installer integration test.

## Findings on the M4 / 16 GB development Mac

The [recorded measurements](measurements-2026-09-15.json) include input hashes,
pinned model revisions, per-stage timings, topology, and qualitative notes.

Twelve scenarios ran successfully at the engineering level: six starter images,
five stress inputs yielding meshes, and one correctly rejected empty image.
Starter Balanced runs took roughly 8–13 seconds end to end; synthetic stress cases
took 8–15 seconds. These are single local observations with warm filesystem/model
caches, not latency guarantees. Peak process RSS was approximately 3.6–4.1 GB; it
does not measure total unified-memory pressure or peak GPU allocation.

| Input | Visual finding | Remaining issue |
| --- | --- | --- |
| Banana photo | Recognizable curved silhouette and stem | Noisy surface, imperfect tips and inferred back |
| Chair | Seat, back and legs recognizable | Uneven legs and lumpy upholstery/frame |
| Teapot | Handle, spout and body present | Soft details and surface ripples |
| Horse | Recognizable overall animal form | Rough anatomy; two mesh components |
| Hamburger | Layers and plate recognizable | Rough surfaces; separate floating component |
| Captured toy photo | Overall toy form retained | Noisy face and body |
| Transparent chair | Comparable geometry to the original | Same reconstruction limits |
| Sideways chair | Rotation retained | Broken/ambiguous thin structures, two components |
| Occluded chair | A valid mesh was returned | Occluder became a large part of the asset; poor reconstruction |
| Two objects | Both silhouettes partly reconstructed | Combined scene rather than a reliable single-object asset |
| Two objects, Keep | A valid mesh was returned | Severe box-like background geometry; unsuitable result |
| Empty transparent PNG | Clear foreground error, no mesh | Expected rejection |

The visual review caught export-material issues: glTF defaults can make vertex-colored
objects metallic, and image-space RGB needs conversion for glTF's linear vertex-color
multiplier. Exports now specify a rough nonmetallic material and convert sampled RGB
to linear values. See the [glTF material specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#materials).
These fixes improve display/export consistency; they do not remove geometric noise.

The next quality work should start with mask preview/correction and a controlled
cleanup stage, then texture baking and a larger held-out photo set (including shoes,
reflective objects, transparent objects, natural clutter, and difficult viewpoints).
The current single-image model cannot reliably recover hidden surfaces or scenes.
