import {
  BufferGeometry,
  Curve,
  Float32BufferAttribute,
  Mesh,
  MeshPhysicalMaterial,
  Vector3,
} from 'three'
import { GLTFExporter } from 'three/addons/exporters/GLTFExporter.js'

export type GeometryQuality = 'draft' | 'balanced' | 'high'

export interface AssetMetadata {
  faces: number
  vertices: number
  materials: number
  textureResolution: string
  format: string
}

const resolution: Record<GeometryQuality, [number, number]> = {
  draft: [96, 12],
  balanced: [192, 24],
  high: [320, 36],
}

/** A local demo asset, deliberately independent of any image or AI engine. */
class SculptureCurve extends Curve<Vector3> {
  constructor(private readonly variation: number) {
    super()
  }

  getPoint(t: number, target = new Vector3()): Vector3 {
    const a = t * Math.PI * 2
    const radius = 1.03 + 0.3 * Math.cos(3 * a)
    return target.set(
      radius * Math.cos(2 * a),
      radius * Math.sin(2 * a) * (1.02 + this.variation * 0.03),
      (0.52 + this.variation * 0.03) * Math.sin(3 * a),
    )
  }
}

/** Indexed, UV-equipped, closed sweep with smoothly varying ceramic-like folds. */
export function createSculptureGeometry(quality: GeometryQuality, seed = 0): BufferGeometry {
  const [segments, sides] = resolution[quality]
  const variation = Math.sin(seed * 0.731)
  const curve = new SculptureCurve(variation)
  const frames = curve.computeFrenetFrames(segments, true)
  const positions: number[] = []
  const uvs: number[] = []
  const indices: number[] = []
  const center = new Vector3()
  const normal = new Vector3()
  const binormal = new Vector3()
  const point = new Vector3()

  for (let i = 0; i <= segments; i++) {
    const t = i / segments
    const angle = t * Math.PI * 2
    curve.getPoint(t, center)
    const thickness = 0.285 + 0.045 * Math.cos(3 * angle + 0.6)
    const twist = Math.sin(angle * 3) * 0.3

    for (let j = 0; j <= sides; j++) {
      // UV seams duplicate vertices, but must share exact positions. Frenet
      // frames use numerical tangents whose endpoints can differ slightly.
      if (i === segments || j === sides) {
        const source = (i === segments ? j : i * (sides + 1)) * 3
        positions.push(positions[source], positions[source + 1], positions[source + 2])
      } else {
        const around = (j / sides) * Math.PI * 2 + twist
        normal.copy(frames.normals[i]).multiplyScalar(Math.cos(around) * thickness * 1.08)
        binormal.copy(frames.binormals[i]).multiplyScalar(Math.sin(around) * thickness * 0.92)
        point.copy(center).add(normal).add(binormal)
        positions.push(point.x, point.y, point.z)
      }
      uvs.push(t, j / sides)
    }
  }

  for (let i = 0; i < segments; i++) {
    for (let j = 0; j < sides; j++) {
      const a = i * (sides + 1) + j
      const b = (i + 1) * (sides + 1) + j
      indices.push(a, a + 1, b, b, a + 1, b + 1)
    }
  }

  const geometry = new BufferGeometry()
  geometry.setAttribute('position', new Float32BufferAttribute(positions, 3))
  geometry.setAttribute('uv', new Float32BufferAttribute(uvs, 2))
  geometry.setIndex(indices)
  geometry.computeVertexNormals()

  // Average normals along both UV seams so the closed form has no lighting seam.
  const normals = geometry.getAttribute('normal')
  const smoothPair = (a: number, b: number) => {
    normal.fromBufferAttribute(normals, a)
    binormal.fromBufferAttribute(normals, b)
    normal.add(binormal).normalize()
    normals.setXYZ(a, normal.x, normal.y, normal.z)
    normals.setXYZ(b, normal.x, normal.y, normal.z)
  }
  for (let i = 0; i <= segments; i++) smoothPair(i * (sides + 1), i * (sides + 1) + sides)
  for (let j = 0; j <= sides; j++) smoothPair(j, segments * (sides + 1) + j)

  geometry.rotateZ(0.17)
  geometry.computeBoundingBox()
  if (geometry.boundingBox) {
    const min = geometry.boundingBox.min.y
    geometry.translate(0, -min + 0.06, 0)
  }
  geometry.computeBoundingBox()
  geometry.computeBoundingSphere()
  geometry.name = 'Sculpt procedural study'
  return geometry
}

export function getAssetMetadata(geometry: BufferGeometry): AssetMetadata {
  return {
    faces: (geometry.index?.count ?? geometry.getAttribute('position').count) / 3,
    vertices: geometry.getAttribute('position').count,
    materials: 1,
    textureResolution: 'None',
    format: 'GLB',
  }
}

export function createSculptureMaterial(color: string): MeshPhysicalMaterial {
  return new MeshPhysicalMaterial({
    color,
    roughness: 0.3,
    metalness: 0.08,
    clearcoat: 0.25,
    clearcoatRoughness: 0.35,
  })
}

/** Export only the asset: no camera, grid, technical overlays, or studio lighting. */
export async function exportSculptureGlb(geometry: BufferGeometry, color: string): Promise<ArrayBuffer> {
  const material = createSculptureMaterial(color)
  const mesh = new Mesh(geometry, material)
  mesh.name = 'Sculpt — procedural demo asset'
  mesh.userData = {
    generator: 'Sculpt prototype',
    source: 'Procedural placeholder. No image reconstruction or AI inference performed.',
  }
  try {
    const result = await new GLTFExporter().parseAsync(mesh, { binary: true })
    if (!(result instanceof ArrayBuffer)) throw new Error('The GLB exporter did not return binary data.')
    return result
  } finally {
    material.dispose()
  }
}
