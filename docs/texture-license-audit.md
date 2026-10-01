# Texture runtime dependency audit — 2026-10-01

Scope: the two additions in the existing hashed Python lockfile, not a new audit
of every previously installed package. No weights or helper models were added.
Sculpt's rasterizer is NumPy/Pillow code; it calls the existing pinned TripoSR
color decoder and does not import OpenGL or nvdiffrast.

| Component | Terms | Evidence |
|---|---|---|
| xatlas-python 0.0.11 | MIT, Markus Worchel | [tagged LICENSE](https://github.com/mworchel/xatlas-python/blob/v0.0.11/LICENSE) |
| vendored xatlas | MIT, Jonathan Young | `extern/xatlas/source/xatlas/xatlas.cpp` header in source distribution |
| xatlas helpers: thekla_atlas, Fast-BVH | MIT, Thekla/NVIDIA Ignacio Castano and Brandon Pelfrey | same source header; this is not nvdiffrast |
| vendored pybind11 | BSD-3-Clause, Wenzel Jakob and contributors | `extern/pybind11/LICENSE` in source distribution |
| fast-simplification 0.2.0 | MIT, PyVista developers | [tagged LICENSE](https://github.com/pyvista/fast-simplification/blob/v0.2.0/LICENSE) |
| Fast-Quadric-Mesh-Simplification | MIT, Sven Forstmann | `fast_simplification/Simplify.h` header in source distribution |
| nanobind binding / robin-map helper | BSD-3-Clause / MIT | [nanobind](https://github.com/wjakob/nanobind/blob/master/LICENSE), [robin-map](https://github.com/Tessil/robin-map/blob/master/LICENSE) |

Inspected the published source archives against PyPI SHA-256:

- xatlas 0.0.11: `72f0bc6c42c19252be87e947d9dfe251c8d6c6943fd43e3d173ddc6b1afad693`.
- fast-simplification 0.2.0: `ed02ea6f4968ec98f963f2d3665dbafd0297ec12aea9e4d0a87c4b3c14d608a8`.


Runtime dependencies remain NumPy; scikit-build-core/CMake, setuptools-scm and
binding build tools are not separately installed by Sculpt when using these
pinned wheels. The fast-simplification CMake file permits optional OpenMP, but
`otool -L` on the installed CPython 3.11 macOS ARM64 extensions showed only
macOS libc++ and libSystem. This inspection does not validate other-platform
wheels or hardware. Binding build versions are not reported by these wheels;
this audit records their license families, not an invented build provenance.

Full notices are in `backend/TEXTURE-NOTICES.txt`, included in the desktop
bundle along with Sculpt's MIT LICENSE. The libraries' own wheel notices also
remain in the local Python environment.
