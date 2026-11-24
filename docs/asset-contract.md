# Engine-Independent Asset Manifest Contract

## 1. Overview

Sculpt generates and refines 3D assets locally across diverse ML inference engines (e.g., TripoSR, procedural demo, future high-end engines) and native post-processing pipelines. To ensure long-term durability, reproducibility, and auditability, each generated asset is accompanied by an **engine-independent, versioned `AssetManifest`** (`manifest.json`) stored directly in its job directory.

The manifest decouples asset inspection, export, and lineage tracking from the specific engine or runtime that produced it:
- An asset remains viewable, inspectable, and exportable (to GLB, STL, OBJ) even if the generating engine or runtime environment is uninstalled or upgraded.
- Full provenance (model revision, weight hashes, hardware backend, driver, generation parameters) is preserved immutably.
- Refinement lineage is explicitly tracked across parent and child asset versions.
- Material and geometry capabilities are truthfully declared without inflated claims (e.g., full PBR vs. diffuse color texture/vertex color).

---

## 2. Manifest Schema (`manifest.json`)

The manifest is stored as formatted JSON adhering to schema version 1.

```json
{
  "schemaVersion": 1,
  "assetId": "e5b8c9a0-1234-4567-89ab-cdef01234567",
  "createdAt": "2026-11-24T12:00:00Z",
  "sourceInputs": {
    "sourceImageId": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
    "sourceImageSha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "maskId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "maskSha256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
    "originalPrompt": null,
    "composedPrompt": null
  },
  "provenance": {
    "engineId": "triposr",
    "engineRevision": "107cefdc244c39106fa830359024f6a2f1c78871",
    "modelWeightHashes": {
      "model.ckpt": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "config.yaml": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    },
    "runtimeVersion": "python-3.11-tsr-v1",
    "backend": "Metal",
    "device": "mps",
    "driverVersion": null
  },
  "parameters": {
    "quality": "balanced",
    "seed": 0,
    "resolution": 128,
    "densityThreshold": 25.0,
    "smoothingIterations": 2,
    "removeSmallComponents": true,
    "targetFaceCount": null,
    "extra": {
      "chunkSize": 4096
    }
  },
  "lineage": {
    "parentAssetId": null,
    "parentAssetVersion": null,
    "operation": "generate",
    "ancestorAssetIds": []
  },
  "geometry": {
    "vertexCount": 24512,
    "faceCount": 48920,
    "componentCount": 1,
    "watertight": false,
    "boundingBox": {
      "min": [-0.52, -0.48, -0.51],
      "max": [0.53, 0.49, 0.52]
    },
    "coordinateSystem": "glTF Y-up",
    "unit": "meters"
  },
  "materials": {
    "channels": [
      "vertex_color",
      "diffuse_texture"
    ],
    "textureWidth": 1024,
    "textureHeight": 1024,
    "textureColorSpace": "sRGB",
    "pbr": false,
    "unknownChannels": []
  },
  "artifacts": [
    {
      "path": "mesh.glb",
      "mimeType": "model/gltf-binary",
      "byteLength": 3456789,
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    },
    {
      "path": "scene_cache.npz",
      "mimeType": "application/x-numpy",
      "byteLength": 12456789,
      "sha256": "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8"
    }
  ],
  "warnings": [
    {
      "code": "NON_MANIFOLD_EDGES",
      "message": "Mesh contains 14 non-manifold edge segments; use Taubin smoothing or slicer repair for 3D printing."
    }
  ]
}
```

---

## 3. Core Contract Principles

### 3.1 Lineage and Non-Destructive Refinement
- **Operation Types:** `generate` (initial inference from image/prompt), `refine` (geometry or texture modification), `export` (conversion to external target format), or `import` (external asset ingested).
- **Lineage Enforcement:** For any operation of type `refine`, `parentAssetId` is **mandatory** and must reference an existing completed asset in the local library.
- **Immutability:** Refinement operations generate a new child asset with its own UUID and manifest; the parent asset and its original artifacts are never mutated.

### 3.2 Geometry and Coordinate Systems
- **Standard Space:** The default coordinate system is **`glTF Y-up`** with units in **`meters`**.
- **Coordinate Conversion:** Exporters (such as STL export) convert `glTF Y-up` to target standards (e.g., STL `Z-up` with physical `millimeters` scale) and record the transform applied.
- **Bounding Box Integrity:** Bounding box bounds `[x, y, z]` must satisfy `min <= max` for all axes, with finite floating point values.

### 3.3 Material Capabilities & Truthful PBR Declarations
- **Channels:** Explicitly enumerates all active channels: `vertex_color`, `diffuse_texture`, `diffuse_color`, `normal`, `roughness`, `metallic`, `ao`, `ambient_occlusion`, `emissive`, `opacity`.
- **Strict PBR Gate:** `pbr` is strictly **`false`** unless all core physically-based rendering channels are present (`normal`, `roughness`, `metallic`, and `diffuse_color` or `diffuse_texture`). TripoSR outputs vertex colors or baked diffuse albedo and is therefore declared with `pbr: false`.
- **Unknown Channels:** Any unconventional or engine-specific material maps must be explicitly enumerated in `unknownChannels` rather than discarded or misrepresented.

### 3.4 Artifact Path Safety
- All artifact paths within `artifacts` must be **relative paths** pointing within the asset's assigned job directory.
- Absolute paths, parent traversal (`..`), hidden path breakouts, and external symlinks are strictly rejected during manifest validation.
- Every artifact must specify its MIME type, byte length, and SHA-256 checksum for tamper detection and data integrity verification.

---

## 4. Migration Policy

For legacy assets created prior to schema version 1 (which stored unstandardized metrics JSON):
1. **Compatibility Wrapper:** The library automatically constructs a synthetic `AssetManifest` v1 on initial load using available metrics and source records.
2. **Deterministic Defaults:** Unspecified coordinate systems default to `glTF Y-up`, unit to `meters`, and `pbr` to `false`.
3. **Integrity Migration:** Missing artifact hashes are computed from local disk files during migration; if a file is missing or corrupted, a warning is recorded in `warnings`.
