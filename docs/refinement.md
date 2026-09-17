# Cached geometry refinement

Generate an image in Sculpt desktop, then use **Refine geometry** in the right
panel. Extraction resolution, surface density, fragment removal, and smoothing
produce a new saved asset. The original remains in the local library. Refinement
does not consume another trial generation, including after the free generation
has been used. Models created before scene caching was introduced must be
generated again once to enable it.

The Python adapter stores a ~2 MB float32 scene representation in a bounded NumPy
archive. Rust verifies its recorded SHA-256 before reuse; Python checks shape,
finite values, model/source revisions, and source identity without loading pickle
objects. A refinement worker constructs only the TripoSR surface decoder and
renderer. It does not load the image encoder or rerun image inference. The
worker remains isolated and cancellable, with a memory-headroom check before
extraction. This is not a warm, persistent model process.

Fragment removal drops disconnected pieces smaller than 0.5% of total surface
area, preserving the largest component. Taubin smoothing applies bounded
shrink/inflate pairs and re-samples colors at the moved vertices. These are
conservative tools, not retopology or texture baking; intentional tiny detached
details may be removed and smoothing can soften edges. Density changes the
surface threshold, so lower values can thicken structures or join nearby parts.

## M4 batch-size measurement, September 17, 2026

We compared isolated workers on the same saved scene, rotating batch order.
The banana comparison used three runs per batch, a 256 grid, density 25, and
no cleanup. The chair comparison used two runs per batch, a 256 grid, density
20, three smoothing iterations, and fragment removal.

| Scene | Batch 4,096: median surface time | Batch 16,384: median surface time | Largest process RSS at 16,384 |
| --- | ---: | ---: | ---: |
| User-provided banana | 18.21 s | 8.78 s | 923.1 MB |
| Upstream example chair | 17.09 s | 9.44 s | 980.2 MB |

Every exported GLB in each comparison was byte-identical. This supports changing
the **cached Metal refinement** default to 16,384 query points per batch. Full
image generation and CPU defaults remain 4,096. The banana's intermediate
8,192 batch measured 12.05 s; the larger tested batch was faster at a similar
process-memory footprint. See the [recorded runs](../backend/benchmarks/refinement-batches-2026-09-17.json).

These are two objects on one M4 MacBook Air with 16 GB memory. Process RSS is not
total peak GPU memory, and timings depend on other applications, temperature,
and available memory. Faster extraction does not improve semantic accuracy.
A 256 grid still has eight times as many samples as 128, so high-resolution
refinement is not instantaneous.

Reproduce from a generated job that contains `metrics.json` and `scene-cache.npz`:

```sh
.sculpt-runtime/venv/bin/python backend/benchmark_refinement.py \
  --parent-job /path/to/saved/job --output backend/outputs/new-batch-run \
  --resolution 256 --repeats 3 --chunks 4096 8192 16384
```

Optional `--density`, `--smoothing`, and `--remove-fragments` apply identical
settings to every batch. The benchmark verifies real GLBs and geometry parity,
records process memory and timings, and refuses to reuse an output directory.
The developer-only worker `queryChunkSize` override is restricted to the three
tested batch sizes. The application never accepts it from frontend requests.
