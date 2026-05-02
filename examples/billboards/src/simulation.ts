// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import { createBillboardFbmMaterial } from "@galeon/three";

/**
 * Live billboard particle.
 *
 * Each particle is its own `THREE.Mesh` with a unique `ShaderMaterial`
 * (so per-particle uniforms — `uLifetimeProgress`, `uColor` — can vary).
 * Three.js shares the compiled GL program across materials with identical
 * source + defines, so 200 materials still produce a single program.
 */
export interface BillboardParticle {
  readonly mesh: THREE.Mesh;
  readonly material: THREE.ShaderMaterial;
  readonly velocity: THREE.Vector3;
  age: number;
  readonly lifetime: number;
}

export interface SpawnBurstOptions {
  /** World-space center of the burst. */
  origin: THREE.Vector3;
  /** Number of particles to spawn. Default: 24. */
  count?: number;
  /** Shared geometry (typically a unit `PlaneGeometry(1, 1)`). */
  geometry: THREE.BufferGeometry;
  /** Scene to add the particle meshes to. */
  scene: THREE.Object3D;
  /** Override RNG for deterministic tests. Defaults to `Math.random`. */
  rng?: () => number;
  /**
   * Lifetime range (seconds). Default: `[1.5, 2.5]`.
   */
  lifetimeRange?: readonly [number, number];
  /**
   * Initial size range (world units). Default: `[0.5, 1.0]`.
   */
  sizeRange?: readonly [number, number];
  /**
   * Upward velocity range. Default: `[1.2, 2.2]`.
   */
  upwardSpeedRange?: readonly [number, number];
  /**
   * Outward velocity max (radial). Default: `1.2`.
   */
  outwardSpeedMax?: number;
}

const DEFAULT_BURST = {
  count: 24,
  lifetimeRange: [1.5, 2.5] as const,
  sizeRange: [0.5, 1.0] as const,
  upwardSpeedRange: [1.2, 2.2] as const,
  outwardSpeedMax: 1.2,
};

/**
 * Spawn a burst of `count` billboard particles at `origin`.
 *
 * Each particle gets:
 * - A random direction in the upward hemisphere (outward radial cone).
 * - A random lifetime, size, and initial vertical speed.
 * - The reference billboard FBM material with `uLifetimeProgress` driven by
 *   the simulation each tick.
 */
export function spawnBurst(opts: SpawnBurstOptions): BillboardParticle[] {
  const count = opts.count ?? DEFAULT_BURST.count;
  const rng = opts.rng ?? Math.random;
  const lifetimeRange = opts.lifetimeRange ?? DEFAULT_BURST.lifetimeRange;
  const sizeRange = opts.sizeRange ?? DEFAULT_BURST.sizeRange;
  const upwardSpeedRange = opts.upwardSpeedRange ?? DEFAULT_BURST.upwardSpeedRange;
  const outwardSpeedMax = opts.outwardSpeedMax ?? DEFAULT_BURST.outwardSpeedMax;

  const out: BillboardParticle[] = [];
  for (let i = 0; i < count; i++) {
    const theta = rng() * Math.PI * 2;
    const radial = rng() * outwardSpeedMax;
    const upward = lerp(upwardSpeedRange[0], upwardSpeedRange[1], rng());
    const velocity = new THREE.Vector3(
      Math.cos(theta) * radial,
      upward,
      Math.sin(theta) * radial,
    );

    const lifetime = lerp(lifetimeRange[0], lifetimeRange[1], rng());
    const size = lerp(sizeRange[0], sizeRange[1], rng());

    const material = createBillboardFbmMaterial({
      color: 0xf2ebd9,
      size,
      noiseScale: 2.0,
      densityFalloff: 0.6,
    });

    const mesh = new THREE.Mesh(opts.geometry, material);
    mesh.position.copy(opts.origin);
    mesh.position.y += 0.05; // lift slightly off the ground
    opts.scene.add(mesh);

    out.push({ mesh, material, velocity, age: 0, lifetime });
  }
  return out;
}

/**
 * Advance every particle by `dt` seconds. Removes and disposes expired
 * particles in place. Returns the number of particles that expired this
 * step.
 *
 * - `position += velocity * dt`
 * - `velocity *= (1 - dt * drag)`
 * - `uLifetimeProgress = age / lifetime`
 *
 * Drag defaults to `0.5` so bursts decelerate visibly without stalling.
 */
export function updateParticles(
  particles: BillboardParticle[],
  dt: number,
  scene: THREE.Object3D,
  drag = 0.5,
): number {
  let expired = 0;
  for (let i = particles.length - 1; i >= 0; i--) {
    const p = particles[i]!;
    p.age += dt;
    if (p.age >= p.lifetime) {
      scene.remove(p.mesh);
      p.material.dispose();
      particles.splice(i, 1);
      expired++;
      continue;
    }
    p.mesh.position.addScaledVector(p.velocity, dt);
    p.velocity.multiplyScalar(Math.max(0, 1 - dt * drag));
    p.material.uniforms.uLifetimeProgress!.value = p.age / p.lifetime;
  }
  return expired;
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
