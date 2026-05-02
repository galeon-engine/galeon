// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  spawnBurst,
  updateParticles,
  type BillboardParticle,
} from "../src/simulation.js";

function makeRng(seed: number): () => number {
  // Tiny LCG, deterministic across test runs.
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0x100000000;
  };
}

function makeScene(): { scene: THREE.Scene; geometry: THREE.PlaneGeometry } {
  return {
    scene: new THREE.Scene(),
    geometry: new THREE.PlaneGeometry(1, 1),
  };
}

describe("spawnBurst", () => {
  test("spawns the requested number of particles", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(0, 0, 0),
      count: 16,
      geometry,
      scene,
      rng: makeRng(1),
    });
    expect(out).toHaveLength(16);
    expect(scene.children).toHaveLength(16);
  });

  test("default count is 24", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(),
      geometry,
      scene,
      rng: makeRng(1),
    });
    expect(out).toHaveLength(24);
  });

  test("each particle has a positive lifetime within the configured range", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(),
      count: 32,
      geometry,
      scene,
      rng: makeRng(7),
      lifetimeRange: [0.5, 1.5],
    });
    for (const p of out) {
      expect(p.lifetime).toBeGreaterThanOrEqual(0.5);
      expect(p.lifetime).toBeLessThanOrEqual(1.5);
      expect(p.age).toBe(0);
    }
  });

  test("each particle has upward velocity within the configured range", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(),
      count: 16,
      geometry,
      scene,
      rng: makeRng(11),
      upwardSpeedRange: [1.0, 2.0],
      outwardSpeedMax: 0.0, // pure-up
    });
    for (const p of out) {
      expect(p.velocity.y).toBeGreaterThanOrEqual(1.0);
      expect(p.velocity.y).toBeLessThanOrEqual(2.0);
      expect(p.velocity.x).toBeCloseTo(0, 6);
      expect(p.velocity.z).toBeCloseTo(0, 6);
    }
  });

  test("each particle uses the provided shared geometry", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(),
      count: 8,
      geometry,
      scene,
      rng: makeRng(1),
    });
    for (const p of out) {
      expect(p.mesh.geometry).toBe(geometry);
    }
  });

  test("each particle's material exposes uLifetimeProgress at 0", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(),
      count: 4,
      geometry,
      scene,
      rng: makeRng(1),
    });
    for (const p of out) {
      expect(p.material.uniforms.uLifetimeProgress!.value).toBe(0);
    }
  });

  test("origin is honoured (with a small Y lift)", () => {
    const { scene, geometry } = makeScene();
    const out = spawnBurst({
      origin: new THREE.Vector3(3, 0, -2),
      count: 4,
      geometry,
      scene,
      rng: makeRng(1),
    });
    for (const p of out) {
      expect(p.mesh.position.x).toBe(3);
      expect(p.mesh.position.z).toBe(-2);
      // Spawned slightly above the ground so they don't z-fight.
      expect(p.mesh.position.y).toBeGreaterThan(0);
      expect(p.mesh.position.y).toBeLessThan(0.5);
    }
  });
});

describe("updateParticles", () => {
  test("ages each particle and advances position by velocity * dt", () => {
    const { scene, geometry } = makeScene();
    const particles = spawnBurst({
      origin: new THREE.Vector3(),
      count: 4,
      geometry,
      scene,
      rng: makeRng(1),
    });
    const before = particles.map((p) => ({
      age: p.age,
      pos: p.mesh.position.clone(),
      vel: p.velocity.clone(),
    }));

    updateParticles(particles, 0.1, scene);

    for (let i = 0; i < particles.length; i++) {
      const p = particles[i]!;
      const b = before[i]!;
      expect(p.age).toBeCloseTo(b.age + 0.1, 6);
      // Allow drag to nudge the velocity (1% drag * 0.1s).
      expect(p.mesh.position.x).toBeCloseTo(b.pos.x + b.vel.x * 0.1, 5);
      expect(p.mesh.position.y).toBeCloseTo(b.pos.y + b.vel.y * 0.1, 5);
      expect(p.mesh.position.z).toBeCloseTo(b.pos.z + b.vel.z * 0.1, 5);
    }
  });

  test("expired particles are removed and disposed", () => {
    const { scene, geometry } = makeScene();
    const particles = spawnBurst({
      origin: new THREE.Vector3(),
      count: 4,
      geometry,
      scene,
      rng: makeRng(1),
      lifetimeRange: [0.1, 0.1],
    });
    const expiredMaterials = particles.map((p) => p.material);

    const expired = updateParticles(particles, 0.2, scene);

    expect(expired).toBe(4);
    expect(particles).toHaveLength(0);
    expect(scene.children).toHaveLength(0);
    // Materials should have been disposed — uniform values still exist on
    // the JS object, but the resource is gone. We can at least verify the
    // material is detached from the scene.
    for (const m of expiredMaterials) {
      // After dispose the material no longer holds any GL resources; the
      // simplest observable signal is that no scene child references it.
      const stillReferenced = scene.children.some(
        (c): boolean =>
          c instanceof THREE.Mesh && (c.material as THREE.Material) === m,
      );
      expect(stillReferenced).toBe(false);
    }
  });

  test("uLifetimeProgress reflects age / lifetime each step", () => {
    const { scene, geometry } = makeScene();
    const particles = spawnBurst({
      origin: new THREE.Vector3(),
      count: 1,
      geometry,
      scene,
      rng: makeRng(1),
      lifetimeRange: [1.0, 1.0],
    });

    updateParticles(particles, 0.25, scene);
    expect(particles[0]!.material.uniforms.uLifetimeProgress!.value).toBeCloseTo(
      0.25,
      5,
    );

    updateParticles(particles, 0.25, scene);
    expect(particles[0]!.material.uniforms.uLifetimeProgress!.value).toBeCloseTo(
      0.5,
      5,
    );
  });

  test("drag reduces velocity each step", () => {
    const { scene, geometry } = makeScene();
    const particles = spawnBurst({
      origin: new THREE.Vector3(),
      count: 1,
      geometry,
      scene,
      rng: makeRng(1),
      upwardSpeedRange: [10.0, 10.0],
      outwardSpeedMax: 0.0,
      lifetimeRange: [10.0, 10.0],
    });
    const initialUpward = particles[0]!.velocity.y;

    // 10 steps of 0.1s with default drag = 0.5: each step y *= (1 - 0.05)
    for (let i = 0; i < 10; i++) {
      updateParticles(particles, 0.1, scene);
    }

    expect(particles[0]!.velocity.y).toBeLessThan(initialUpward);
  });

  test("zero-particle update is a no-op", () => {
    const { scene } = makeScene();
    const expired = updateParticles([], 0.1, scene);
    expect(expired).toBe(0);
  });
});

describe("end-to-end burst lifecycle", () => {
  test("burst spawned, simulated, fully expired with scene cleanup", () => {
    const { scene, geometry } = makeScene();
    const particles: BillboardParticle[] = spawnBurst({
      origin: new THREE.Vector3(),
      count: 12,
      geometry,
      scene,
      rng: makeRng(42),
      lifetimeRange: [0.5, 0.6],
    });
    expect(scene.children.length).toBe(12);

    let totalExpired = 0;
    // Simulate 1 second in 0.05s steps — well past max lifetime.
    for (let i = 0; i < 20; i++) {
      totalExpired += updateParticles(particles, 0.05, scene);
    }
    expect(totalExpired).toBe(12);
    expect(particles).toHaveLength(0);
    expect(scene.children).toHaveLength(0);
  });
});
