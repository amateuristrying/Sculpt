import { Component, useEffect, useMemo, useRef, type ComponentRef, type ErrorInfo, type ReactNode } from 'react'
import { Canvas, useThree } from '@react-three/fiber'
import {
  ContactShadows,
  Environment,
  GizmoHelper,
  GizmoViewport,
  Grid,
  Lightformer,
  OrbitControls,
} from '@react-three/drei'
import { MOUSE, TOUCH, type BufferGeometry, type Material, type MeshPhysicalMaterial } from 'three'
import type { ImportedAsset } from '../geometry/importedAsset'
import {
  createSculptureGeometry,
  createSculptureMaterial,
  exportSculptureGlb,
  getAssetMetadata,
  type AssetMetadata,
  type GeometryQuality,
} from '../geometry/sculpture'

export type { AssetMetadata, GeometryQuality } from '../geometry/sculpture'
export type ViewportMode = 'material' | 'wireframe' | 'points' | 'technical'

export interface ViewportApi {
  exportGlb: () => Promise<ArrayBuffer>
  capturePreview: () => string
}

export interface ViewportProps {
  mode: ViewportMode
  grid: boolean
  autoRotate: boolean
  lightIntensity: number
  materialColor: string
  resetKey: number
  navigationMode?: 'orbit' | 'pan'
  assetSeed: number
  importedAsset?: ImportedAsset | null
  geometryQuality: GeometryQuality
  onMetadata: (metadata: AssetMetadata) => void
  onReady?: (api: ViewportApi) => void
}

const CAMERA_POSITION: [number, number, number] = [4.2, 3.0, 7.3]
const CAMERA_TARGET: [number, number, number] = [0, 1.6, 0]

function Asset({ geometry, material, mode }: {
  geometry: BufferGeometry
  material: Material | Material[]
  mode: ViewportMode
}) {
  return (
    <group>
      {mode === 'material' && <mesh geometry={geometry} material={material} castShadow receiveShadow />}
      {mode === 'wireframe' && (
        <>
          <mesh geometry={geometry}>
            <meshBasicMaterial color="#191c1c" polygonOffset polygonOffsetFactor={1} polygonOffsetUnits={1} />
          </mesh>
          <mesh geometry={geometry}>
            <meshBasicMaterial color="#b7c0bb" wireframe transparent opacity={0.55} />
          </mesh>
        </>
      )}
      {mode === 'technical' && (
        <>
          <mesh geometry={geometry} castShadow>
            <meshStandardMaterial color="#252b28" roughness={0.83} metalness={0.2} polygonOffset polygonOffsetFactor={1} polygonOffsetUnits={1} />
          </mesh>
          <mesh geometry={geometry}>
            <meshBasicMaterial color="#b1bcb4" wireframe transparent opacity={0.25} />
          </mesh>
          <points geometry={geometry}>
            <pointsMaterial color="#e2e8df" size={0.012} sizeAttenuation transparent opacity={0.72} />
          </points>
        </>
      )}
      {mode === 'points' && (
        <points geometry={geometry}>
          <pointsMaterial color="#e2e9de" size={0.018} sizeAttenuation transparent opacity={0.78} depthWrite={false} />
        </points>
      )}
    </group>
  )
}

function Scene({ geometry, material, ...props }: ViewportProps & {
  geometry: BufferGeometry
  material: MeshPhysicalMaterial
}) {
  const controls = useRef<ComponentRef<typeof OrbitControls>>(null)
  const { camera, gl, scene, invalidate } = useThree()
  const readyCallback = useRef(props.onReady)
  readyCallback.current = props.onReady

  useEffect(() => {
    camera.position.set(...CAMERA_POSITION)
    controls.current?.target.set(...CAMERA_TARGET)
    controls.current?.update()
    invalidate()
  }, [props.resetKey, camera, invalidate])

  useEffect(() => {
    readyCallback.current?.({
      exportGlb: () => props.importedAsset ? Promise.resolve(props.importedAsset.bytes.slice(0)) : exportSculptureGlb(geometry, props.materialColor),
      capturePreview: () => {
        gl.render(scene, camera)
        return gl.domElement.toDataURL('image/png')
      },
    })
  }, [geometry, props.materialColor, props.importedAsset, gl, scene, camera])

  const light = Math.max(0.1, props.lightIntensity)

  return (
    <>
      <color attach="background" args={['#181b1a']} />
      <fog attach="fog" args={['#181b1a', 12, 35]} />
      <ambientLight intensity={0.5 * light} />
      <directionalLight position={[-3, 6, 5]} intensity={2.7 * light} color="#fff9eb" />
      <directionalLight position={[5, 3, -4]} intensity={2.1 * light} color="#dce8ec" />
      <directionalLight position={[-4, 1, -2]} intensity={0.8 * light} color="#dce9dc" />
      <Environment resolution={128} frames={1} environmentIntensity={0.6 * light}>
        <Lightformer form="rect" intensity={3} color="#ffffff" position={[-5, 3, 1]} rotation={[0, Math.PI / 2, 0]} scale={[4, 7, 1]} />
        <Lightformer form="rect" intensity={2} color="#fff8e7" position={[1, 5, -2]} rotation={[Math.PI / 2, 0, 0]} scale={[5, 4, 1]} />
        <Lightformer form="rect" intensity={3} color="#e0e9ed" position={[4, 2, 0]} rotation={[0, -Math.PI / 2, 0]} scale={[2, 5, 1]} />
      </Environment>
      <group>{props.importedAsset
        ? props.importedAsset.parts.map((part, index) => <Asset key={index} {...part} mode={props.mode} />)
        : <Asset geometry={geometry} material={material} mode={props.mode} />}</group>
      {/* Keep shadow render targets alive across display-mode changes. */}
      <ContactShadows
        key={props.importedAsset?.metadata.vertices ?? props.assetSeed}
        position={[0, 0, 0]}
        visible={props.mode === 'material'}
        frames={props.mode === 'material' ? 1 : 0}
        opacity={0.42}
        scale={10}
        blur={2.8}
        far={4}
        resolution={512}
        color="#000000"
      />
      {(props.grid || props.mode === 'technical') && (
        <Grid
          position={[0, -0.01, 0]}
          args={[30, 30]}
          cellSize={0.25}
          cellThickness={props.mode === 'technical' ? 0.65 : 0.5}
          cellColor={props.mode === 'technical' ? '#48514b' : '#39413b'}
          sectionSize={1}
          sectionThickness={props.mode === 'technical' ? 0.8 : 0.7}
          sectionColor={props.mode === 'technical' ? '#606a62' : '#4b554d'}
          fadeDistance={20}
          fadeStrength={1.8}
          infiniteGrid
          followCamera={false}
        />
      )}
      <OrbitControls
        ref={controls}
        makeDefault
        target={CAMERA_TARGET}
        enableDamping
        dampingFactor={0.085}
        rotateSpeed={0.65}
        panSpeed={0.7}
        zoomSpeed={0.75}
        minDistance={2.2}
        maxDistance={20}
        maxPolarAngle={Math.PI * 0.89}
        autoRotate={props.autoRotate}
        autoRotateSpeed={0.6}
        mouseButtons={{ LEFT: props.navigationMode === 'pan' ? MOUSE.PAN : MOUSE.ROTATE, MIDDLE: MOUSE.DOLLY, RIGHT: MOUSE.PAN }}
        touches={{ ONE: props.navigationMode === 'pan' ? TOUCH.PAN : TOUCH.ROTATE, TWO: TOUCH.DOLLY_PAN }}
      />
      <GizmoHelper alignment="bottom-right" margin={[48, 48]} renderPriority={1}>
        <GizmoViewport axisColors={['#ae8178', '#a1b29b', '#859bab']} labelColor="#1b211d" axisHeadScale={0.7} hideNegativeAxes />
      </GizmoHelper>
    </>
  )
}

function WebGLFallback() {
  return (
    <div role="status" style={{ height: '100%', minHeight: 320, display: 'grid', placeContent: 'center', textAlign: 'center', gap: 10, padding: 40, color: '#adb7ad', fontSize: 13 }}>
      <strong style={{ color: '#e0e6dd', fontWeight: 500 }}>The 3D viewport is unavailable</strong>
      <span>Sculpt requires a WebGL 2 compatible graphics context.</span>
      <span>Your image and project controls remain available.</span>
    </div>
  )
}

class ViewportBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() { return { failed: true } }
  componentDidCatch(error: Error, info: ErrorInfo) { console.error('Sculpt viewport:', error, info.componentStack) }
  render() { return this.state.failed ? <WebGLFallback /> : this.props.children }
}

function useDisposeOnRelease(resource: { dispose: () => void } | null) {
  const active = useRef<typeof resource | null>(null)
  useEffect(() => {
    active.current = resource
    return () => {
      active.current = null
      // StrictMode immediately replays effects. Keep a resource if that replay
      // still uses it; dispose replaced/unmounted GPU resources once released.
      queueMicrotask(() => {
        if (active.current !== resource) resource?.dispose()
      })
    }
  }, [resource])
}

export default function Viewport(props: ViewportProps) {
  const geometry = useMemo(() => createSculptureGeometry(props.geometryQuality, props.assetSeed), [props.geometryQuality, props.assetSeed])
  const material = useMemo(() => createSculptureMaterial(props.materialColor), [props.materialColor])
  const metadataCallback = useRef(props.onMetadata)
  metadataCallback.current = props.onMetadata

  useEffect(() => { metadataCallback.current(props.importedAsset?.metadata ?? getAssetMetadata(geometry)) }, [geometry, props.importedAsset])
  // These shared objects are supplied as props, so this component owns their
  // lifetime. R3F independently disposes each view's declarative overlay material.
  useDisposeOnRelease(geometry)
  useDisposeOnRelease(material)
  useDisposeOnRelease(props.importedAsset ?? null)

  return (
    <div className="viewport-canvas" style={{ width: '100%', height: '100%', touchAction: 'none' }} aria-label="Interactive 3D asset viewport. Drag to orbit, right-drag to pan, scroll to zoom.">
      <ViewportBoundary>
        <Canvas
          camera={{ position: CAMERA_POSITION, fov: 35, near: 0.1, far: 100 }}
          dpr={[1, 2]}
          frameloop={props.autoRotate ? 'always' : 'demand'}
          gl={{ antialias: true, alpha: false, preserveDrawingBuffer: true, powerPreference: 'high-performance' }}
          // Offscreen contact-shadow passes need transparent clears even though
          // the visible scene itself has an opaque background.
          onCreated={({ gl }) => gl.setClearAlpha(0)}
          fallback={<WebGLFallback />}
        >
          <Scene {...props} geometry={geometry} material={material} />
        </Canvas>
      </ViewportBoundary>
    </div>
  )
}
