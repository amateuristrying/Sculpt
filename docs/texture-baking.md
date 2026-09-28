# Local color texture baking

Refine can create a textured GLB from a saved Sculpt reconstruction. The
operation runs after cached scene extraction, so it does not run image inference
again or spend another generation. Vertex colors remain the default because
they are faster and preserve the original mesh exactly.

Selecting 1K or 2K performs three bounded steps:

1. An optional face target is applied with fast-simplification 0.2.0.
2. UV islands are generated with xatlas 0.0.11.
3. Sculpt's CPU rasterizer interpolates the saved reconstruction colors into
   the UV atlas, pads seams, and embeds the PNG in the GLB.

The baker does not use OpenGL, nvdiffrast, cloud inference, or a restricted
background-removal helper. The two runtime packages are permissively licensed:
[xatlas-python](https://github.com/mworchel/xatlas-python) is MIT and
[fast-simplification](https://github.com/pyvista/fast-simplification) is MIT.
Their transitive licenses remain recorded by the pinned runtime installation.

This is a color bake from TripoSR's predicted surface colors, not a new texture
generation model. It improves interchange and reduces vertex count, but cannot
invent image detail that the reconstruction did not infer. Texture output is
currently GLB-only; OBJ/MTL and standalone image export are future work.
