import { afterEach, describe, expect, it, vi } from 'vitest'
import { Box3, BoxGeometry, DataTexture, Float32BufferAttribute, Group, Mesh, MeshStandardMaterial, Vector3 } from 'three'
import { GLTFLoader, type GLTF } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { GLTFExporter } from 'three/examples/jsm/exporters/GLTFExporter.js'
import { parseGeneratedGlb } from '../geometry/importedAsset'

class BlobReader {
  result: ArrayBuffer | null = null
  onloadend?: () => void
  readAsArrayBuffer(blob: Blob) { void blob.arrayBuffer().then(bytes => { this.result = bytes; this.onloadend?.() }) }
}
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks() })

describe('reconstructed GLB display boundary', () => {
  it('preserves export bytes, source colors and every mesh while fitting the viewport', async () => {
    vi.stubGlobal('FileReader', BlobReader)
    const geometry = new BoxGeometry(2, 1, 1)
    geometry.setAttribute('color', new Float32BufferAttribute(new Float32Array(geometry.getAttribute('position').count * 3).fill(.7), 3))
    const material = new MeshStandardMaterial({ vertexColors: true })
    const scene = new Group()
    const first = new Mesh(geometry, material)
    const second = new Mesh(geometry, material)
    first.position.set(10, 4, 0); second.position.set(14, 4, 0)
    scene.add(first, second)
    const bytes = await new GLTFExporter().parseAsync(scene, { binary: true }) as ArrayBuffer
    const before = new Uint8Array(bytes).slice()
    const asset = await parseGeneratedGlb(bytes)
    try {
      expect(asset.parts).toHaveLength(2)
      expect(asset.metadata).toMatchObject({ faces: 24, materials: 1, textureResolution: 'Vertex colors' })
      expect(asset.parts.every(part => part.geometry.getAttribute('color').count > 0)).toBe(true)
      expect(new Uint8Array(asset.bytes)).toEqual(before)
      const bounds = new Box3()
      for (const part of asset.parts) { part.geometry.computeBoundingBox(); bounds.union(part.geometry.boundingBox!) }
      expect(bounds.min.y).toBeCloseTo(.08)
      expect(bounds.getCenter(new Vector3()).x).toBeCloseTo(0)
      expect(bounds.getSize(new Vector3()).x).toBeCloseTo(3.3)
    } finally { asset.dispose(); geometry.dispose(); material.dispose() }
  })
  it('shows embedded texture dimensions and releases shared GPU textures once', async () => {
    const map = new DataTexture(new Uint8Array(4 * 4 * 4), 4, 4)
    const dispose = vi.spyOn(map, 'dispose')
    const material = new MeshStandardMaterial({ map })
    const scene = new Group()
    scene.add(new Mesh(new BoxGeometry(), material), new Mesh(new BoxGeometry(), material))
    vi.spyOn(GLTFLoader.prototype, 'parseAsync').mockResolvedValue({ scene } as unknown as GLTF)
    const asset = await parseGeneratedGlb(new ArrayBuffer(0))
    expect(asset.metadata.textureResolution).toBe('4 × 4')
    expect((asset.parts[0].material as MeshStandardMaterial).map).toBe(map)
    asset.dispose()
    expect(dispose).toHaveBeenCalledTimes(1)
  })
  it('rejects malformed output instead of displaying a sample', async () => {
    await expect(parseGeneratedGlb(new ArrayBuffer(32))).rejects.toThrow()
  })
})
