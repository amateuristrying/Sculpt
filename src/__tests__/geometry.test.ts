import { describe, expect, it } from 'vitest'
import { Vector3 } from 'three'
import { createSculptureGeometry, getAssetMetadata, type GeometryQuality } from '../geometry/sculpture'

describe('exportable procedural asset', () => {
  it.each<GeometryQuality>(['draft', 'balanced', 'high'])('%s geometry has finite, non-degenerate triangles and unit normals', quality => {
    const geometry = createSculptureGeometry(quality, 42)
    try {
      const positions = geometry.getAttribute('position')
      const normals = geometry.getAttribute('normal')
      const uvs = geometry.getAttribute('uv')
      const index = geometry.getIndex()!
      expect(index.count % 3).toBe(0)
      expect(normals.count).toBe(positions.count)
      expect(uvs.count).toBe(positions.count)
      expect([...positions.array, ...normals.array, ...uvs.array].every(Number.isFinite)).toBe(true)
      expect([...uvs.array].every(value => value >= 0 && value <= 1)).toBe(true)
      expect([...index.array].every(value => Number.isInteger(value) && value >= 0 && value < positions.count)).toBe(true)

      const a = new Vector3()
      const b = new Vector3()
      const c = new Vector3()
      let smallestTriangle = Infinity
      for (let i = 0; i < index.count; i += 3) {
        a.fromBufferAttribute(positions, index.getX(i))
        b.fromBufferAttribute(positions, index.getX(i + 1)).sub(a)
        c.fromBufferAttribute(positions, index.getX(i + 2)).sub(a)
        smallestTriangle = Math.min(smallestTriangle, b.cross(c).lengthSq())
      }
      expect(smallestTriangle).toBeGreaterThan(1e-12)
      for (let i = 0; i < normals.count; i++) {
        a.fromBufferAttribute(normals, i)
        expect(a.length()).toBeCloseTo(1, 5)
      }
      expect(geometry.boundingBox!.min.y).toBeCloseTo(0.06, 5)
      expect(geometry.boundingSphere!.radius).toBeGreaterThan(0)
    } finally {
      geometry.dispose()
    }
  })

  it('metadata agrees with the geometry shared by mesh and point views; higher quality adds detail', () => {
    const geometries = (['draft', 'balanced', 'high'] as const).map(quality => createSculptureGeometry(quality))
    try {
      const metadata = geometries.map(getAssetMetadata)
      geometries.forEach((geometry, i) => {
        expect(metadata[i].faces).toBe(geometry.index!.count / 3)
        expect(metadata[i].vertices).toBe(geometry.getAttribute('position').count)
        expect(metadata[i]).toMatchObject({ materials: 1, textureResolution: 'None', format: 'GLB' })
      })
      expect(metadata[0].faces).toBeLessThan(metadata[1].faces)
      expect(metadata[1].faces).toBeLessThan(metadata[2].faces)
      expect(metadata[0].vertices).toBeLessThan(metadata[1].vertices)
      expect(metadata[1].vertices).toBeLessThan(metadata[2].vertices)
    } finally {
      geometries.forEach(geometry => geometry.dispose())
    }
  })

  it('reproduces a fixture from its seed and keeps both UV seams geometrically closed', () => {
    const geometry = createSculptureGeometry('draft', 17)
    const repeated = createSculptureGeometry('draft', 17)
    const different = createSculptureGeometry('draft', 18)
    try {
      const positions = geometry.getAttribute('position')
      const uvs = geometry.getAttribute('uv')
      expect(positions.array).toEqual(repeated.getAttribute('position').array)
      expect(positions.array).not.toEqual(different.getAttribute('position').array)
      // Find seam partners by UV rather than coupling this test to tessellation counts.
      const starts = new Map<string, number>()
      const point = new Vector3()
      const partner = new Vector3()
      for (let i = 0; i < uvs.count; i++) {
        const u = uvs.getX(i)
        const v = uvs.getY(i)
        if (u === 0) starts.set(`u:${v}`, i)
        if (v === 0) starts.set(`v:${u}`, i)
      }
      for (let i = 0; i < uvs.count; i++) {
        const u = uvs.getX(i)
        const v = uvs.getY(i)
        const matched = u === 1 ? starts.get(`u:${v}`) : v === 1 ? starts.get(`v:${u}`) : undefined
        if (matched === undefined) continue
        point.fromBufferAttribute(positions, i)
        partner.fromBufferAttribute(positions, matched)
        expect(point.distanceTo(partner)).toBeLessThan(1e-5)
      }
    } finally {
      geometry.dispose()
      repeated.dispose()
      different.dispose()
    }
  })
})
