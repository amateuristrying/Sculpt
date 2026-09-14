import { Box3, BufferGeometry, Material, Matrix4, Mesh, Vector3 } from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import type { AssetMetadata } from './sculpture'

export interface ImportedAsset {
  bytes: ArrayBuffer
  parts: { geometry: BufferGeometry; material: Material | Material[] }[]
  metadata: AssetMetadata
  dispose(): void
}

/** The harness returns a contained GLB. Display normalization never changes export bytes. */
export async function parseGeneratedGlb(bytes: ArrayBuffer): Promise<ImportedAsset> {
  const gltf = await new GLTFLoader().parseAsync(bytes, '')
  const parts: ImportedAsset['parts'] = []
  const materials = new Set<Material>()
  gltf.scene.updateMatrixWorld(true)
  gltf.scene.traverse(object => {
    if (!(object instanceof Mesh)) return
    const geometry = object.geometry.clone().applyMatrix4(object.matrixWorld)
    parts.push({ geometry, material: object.material })
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) materials.add(material)
  })
  // Cloned geometries above own all display buffers from here on.
  gltf.scene.traverse(object => { if (object instanceof Mesh) object.geometry.dispose() })
  if (!parts.length) throw new Error('The generated asset has no mesh')
  const bounds = new Box3()
  for (const { geometry } of parts) { geometry.computeBoundingBox(); bounds.union(geometry.boundingBox!) }
  const center = bounds.getCenter(new Vector3())
  const size = bounds.getSize(new Vector3())
  const scale = 3.3 / Math.max(size.x, size.y, size.z, 0.001)
  const transform = new Matrix4().makeTranslation(-center.x * scale, -bounds.min.y * scale + 0.08, -center.z * scale)
    .multiply(new Matrix4().makeScale(scale, scale, scale))
  for (const { geometry } of parts) geometry.applyMatrix4(transform)
  const metadata: AssetMetadata = {
    vertices: parts.reduce((sum, part) => sum + part.geometry.getAttribute('position').count, 0),
    faces: parts.reduce((sum, part) => sum + (part.geometry.index?.count ?? part.geometry.getAttribute('position').count) / 3, 0),
    materials: materials.size,
    textureResolution: parts.some(part => part.geometry.hasAttribute('color')) ? 'Vertex colors' : 'None',
    format: 'GLB',
  }
  return { bytes, parts, metadata, dispose() {
    parts.forEach(part => part.geometry.dispose())
    materials.forEach(material => material.dispose())
  } }
}
