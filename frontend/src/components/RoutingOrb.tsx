import { useRef } from 'react'
import { useTranslation } from '../workspace'
import { Canvas, useFrame } from '@react-three/fiber'
import { Float, MeshDistortMaterial } from '@react-three/drei'
import * as THREE from 'three'

function Orbit({ radius, tilt, speed, offset }: { radius: number; tilt: number; speed: number; offset: number }) {
  const particle = useRef<THREE.Mesh>(null)
  const points = Array.from({ length: 100 }, (_, i) => {
    const angle = (i / 99) * Math.PI * 2
    return new THREE.Vector3(Math.cos(angle) * radius, Math.sin(angle) * radius, 0)
  })
  const geometry = new THREE.BufferGeometry().setFromPoints(points)

  useFrame(({ clock }) => {
    if (particle.current) {
      const angle = clock.elapsedTime * speed + offset
      particle.current.position.set(Math.cos(angle) * radius, Math.sin(angle) * radius, 0)
    }
  })

  return (
    <group rotation={[tilt, 0.35, -0.45]}>
      <primitive object={new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: '#9bd58c', transparent: true, opacity: 0.3 }))} />
      <mesh ref={particle}>
        <sphereGeometry args={[0.065, 16, 16]} />
        <meshBasicMaterial color="#dcffc8" />
      </mesh>
    </group>
  )
}

function Orb() {
  const core = useRef<THREE.Mesh>(null)
  useFrame(({ clock }) => {
    if (core.current) core.current.rotation.y = clock.elapsedTime * 0.1
  })

  return (
    <Float speed={1.3} rotationIntensity={0.1} floatIntensity={0.35}>
      <mesh ref={core}>
        <icosahedronGeometry args={[1.18, 20]} />
        <MeshDistortMaterial
          color="#a3c887"
          roughness={0.24}
          metalness={0.5}
          distort={0.3}
          speed={1.5}
        />
      </mesh>
      <mesh scale={1.015}>
        <icosahedronGeometry args={[1.18, 3]} />
        <meshBasicMaterial color="#d3f6bc" wireframe transparent opacity={0.075} />
      </mesh>
      <Orbit radius={1.64} tilt={0.9} speed={0.26} offset={0} />
      <Orbit radius={1.87} tilt={-0.4} speed={-0.18} offset={2.4} />
      <Orbit radius={2.08} tilt={1.4} speed={0.14} offset={4.8} />
    </Float>
  )
}

export default function RoutingOrb() {
  return (
    <div className="routing-orb" aria-label={useTranslation()('routing')}>
      <Canvas camera={{ position: [0, 0, 5.7], fov: 45 }} dpr={[1, 1.5]} gl={{ alpha: true, antialias: true }}>
        <ambientLight intensity={1.3} />
        <directionalLight position={[3, 5, 5]} intensity={3.5} color="#e4ffd1" />
        <directionalLight position={[-4, -2, 1]} intensity={2.2} color="#517b42" />
        <Orb />
      </Canvas>
    </div>
  )
}
