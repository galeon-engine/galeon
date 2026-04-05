// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test, spyOn } from "bun:test";
import * as THREE from "three";
import { RendererCache } from "../src/renderer-cache.js";
import { TRANSFORM_STRIDE, type FramePacketView } from "../src/types.js";

function makeTransforms(entityCount: number): Float32Array {
  const transforms = new Float32Array(entityCount * TRANSFORM_STRIDE);
  for (let i = 0; i < entityCount; i++) {
    const off = i * TRANSFORM_STRIDE;
    transforms[off + 6] = 1;
    transforms[off + 7] = 1;
    transforms[off + 8] = 1;
    transforms[off + 9] = 1;
  }
  return transforms;
}

describe("RendererCache custom channels", () => {
  test("loads each channel payload once per frame", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const dataCalls = new Map<string, number>();
    const strideCalls = new Map<string, number>();
    let nameCalls = 0;

    const packet: FramePacketView = {
      entity_count: 2,
      entity_ids: new Uint32Array([1, 2]),
      entity_generations: new Uint32Array([0, 0]),
      transforms: makeTransforms(2),
      visibility: new Uint8Array([1, 1]),
      mesh_handles: new Uint32Array([10, 10]),
      material_handles: new Uint32Array([20, 20]),
      custom_channel_count: 2,
      custom_channel_name_at(index: number): string {
        nameCalls += 1;
        return index === 0 ? "heat" : "tint";
      },
      custom_channel_stride(name: string): number {
        strideCalls.set(name, (strideCalls.get(name) ?? 0) + 1);
        return name === "heat" ? 1 : 2;
      },
      custom_channel_data(name: string): Float32Array {
        dataCalls.set(name, (dataCalls.get(name) ?? 0) + 1);
        return name === "heat"
          ? new Float32Array([0.25, 0.75])
          : new Float32Array([1, 2, 3, 4]);
      },
    };

    cache.applyFrame(packet);

    expect(nameCalls).toBe(2);
    expect(strideCalls.get("heat")).toBe(1);
    expect(strideCalls.get("tint")).toBe(1);
    expect(dataCalls.get("heat")).toBe(1);
    expect(dataCalls.get("tint")).toBe(1);

    const first = cache.getObject(1, 0);
    const second = cache.getObject(2, 0);
    expect(first?.userData.heat).toBe(0.25);
    expect(Array.from(first?.userData.tint ?? [])).toEqual([1, 2]);
    expect(second?.userData.heat).toBe(0.75);
    expect(Array.from(second?.userData.tint ?? [])).toEqual([3, 4]);
  });
});

// ---------------------------------------------------------------------------
// Helpers for handle-tracking tests
// ---------------------------------------------------------------------------

function makePacket(overrides: Partial<FramePacketView> & { entity_count: number }): FramePacketView {
  const n = overrides.entity_count;
  return {
    entity_ids: new Uint32Array(n),
    entity_generations: new Uint32Array(n),
    transforms: makeTransforms(n),
    visibility: new Uint8Array(n).fill(1),
    mesh_handles: new Uint32Array(n),
    material_handles: new Uint32Array(n),
    custom_channel_count: 0,
    custom_channel_name_at: () => "",
    custom_channel_stride: () => 0,
    custom_channel_data: () => new Float32Array(0),
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// T3-A: Consumer material override survives applyFrame
// ---------------------------------------------------------------------------

describe("RendererCache handle-based tracking", () => {
  test("consumer material override survives subsequent frames", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const registeredMat = new THREE.MeshBasicMaterial({ color: 0x00ff00 });
    cache.registerMaterial(1, registeredMat);
    cache.registerGeometry(1, new THREE.BoxGeometry());

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([42]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    });

    // Frame 1: entity created with registered material
    cache.applyFrame(packet);
    const obj = cache.getObject(42, 0)!;
    expect(obj.material).toBe(registeredMat);

    // Consumer overrides material (e.g. multi-material array for per-face texturing)
    const customMat = new THREE.MeshBasicMaterial({ color: 0xff0000 });
    obj.material = customMat;

    // Frame 2: same handles — override must survive
    cache.applyFrame(packet);
    expect(obj.material).toBe(customMat);

    // Frame 3: still survives
    cache.applyFrame(packet);
    expect(obj.material).toBe(customMat);
  });

  // ---------------------------------------------------------------------------
  // T3-B: Consumer geometry override survives applyFrame
  // ---------------------------------------------------------------------------

  test("consumer geometry override survives subsequent frames", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const registeredGeo = new THREE.BoxGeometry();
    cache.registerGeometry(5, registeredGeo);
    cache.registerMaterial(5, new THREE.MeshBasicMaterial());

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([7]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([5]),
      material_handles: new Uint32Array([5]),
    });

    cache.applyFrame(packet);
    const obj = cache.getObject(7, 0)!;
    expect(obj.geometry).toBe(registeredGeo);

    // Consumer overrides geometry
    const customGeo = new THREE.SphereGeometry(1);
    obj.geometry = customGeo;

    // Frame 2: same handle — override survives
    cache.applyFrame(packet);
    expect(obj.geometry).toBe(customGeo);
  });

  // ---------------------------------------------------------------------------
  // T3-C: Handle change does reassign material/geometry
  // ---------------------------------------------------------------------------

  test("changing handle reassigns material and geometry", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const matA = new THREE.MeshBasicMaterial({ color: 0x00ff00 });
    const matB = new THREE.MeshBasicMaterial({ color: 0x0000ff });
    const geoA = new THREE.BoxGeometry();
    const geoB = new THREE.SphereGeometry(1);
    cache.registerMaterial(1, matA);
    cache.registerMaterial(2, matB);
    cache.registerGeometry(1, geoA);
    cache.registerGeometry(2, geoB);

    const packet1 = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([10]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    });

    cache.applyFrame(packet1);
    const obj = cache.getObject(10, 0)!;
    expect(obj.material).toBe(matA);
    expect(obj.geometry).toBe(geoA);

    // Frame 2: handles change to 2
    const packet2 = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([10]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([2]),
      material_handles: new Uint32Array([2]),
    });

    cache.applyFrame(packet2);
    expect(obj.material).toBe(matB);
    expect(obj.geometry).toBe(geoB);
  });

  // ---------------------------------------------------------------------------
  // T3-D: Warning fires once for unregistered handle
  // ---------------------------------------------------------------------------

  test("warns once per entity for unregistered handles", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const warnSpy = spyOn(console, "warn").mockImplementation(() => {});

    // No geometries or materials registered — handle 99 is unknown
    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([99]),
      material_handles: new Uint32Array([88]),
    });

    // Frame 1: should warn
    cache.applyFrame(packet);
    expect(warnSpy).toHaveBeenCalledTimes(2); // one for mesh, one for material

    // Frame 2: same entity, same handles — no new warnings
    warnSpy.mockClear();
    cache.applyFrame(packet);
    expect(warnSpy).toHaveBeenCalledTimes(0);

    // Remove entity, then re-add — warning should fire again
    const emptyPacket = makePacket({ entity_count: 0 });
    cache.applyFrame(emptyPacket);

    warnSpy.mockClear();
    cache.applyFrame(packet);
    expect(warnSpy).toHaveBeenCalledTimes(2);

    warnSpy.mockRestore();
  });

  // ---------------------------------------------------------------------------
  // T3-E: Entity removal clears all tracking state
  // ---------------------------------------------------------------------------

  test("entity removal clears handle and warning tracking state", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const mat = new THREE.MeshBasicMaterial();
    const geo = new THREE.BoxGeometry();
    cache.registerMaterial(1, mat);
    cache.registerGeometry(1, geo);

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([50]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    });

    cache.applyFrame(packet);
    expect(cache.objectCount).toBe(1);

    // Remove entity by omitting from next frame
    const emptyPacket = makePacket({ entity_count: 0 });
    cache.applyFrame(emptyPacket);
    expect(cache.objectCount).toBe(0);

    // Re-add same entityId with different handles — should work cleanly
    const mat2 = new THREE.MeshBasicMaterial({ color: 0x0000ff });
    const geo2 = new THREE.SphereGeometry(1);
    cache.registerMaterial(2, mat2);
    cache.registerGeometry(2, geo2);

    const packet2 = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([50]),
      entity_generations: new Uint32Array([1]),
      mesh_handles: new Uint32Array([2]),
      material_handles: new Uint32Array([2]),
    });

    cache.applyFrame(packet2);
    expect(cache.objectCount).toBe(1);
    const obj = cache.getObject(50, 1)!;
    expect(obj.material).toBe(mat2);
    expect(obj.geometry).toBe(geo2);
  });

  // ---------------------------------------------------------------------------
  // T3-F: Stale-generation eviction resets all tracking state
  // ---------------------------------------------------------------------------

  test("generation mismatch evicts stale object and resets handle/warning state", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const warnSpy = spyOn(console, "warn").mockImplementation(() => {});

    // Gen 0: entity 5 with unregistered handle 99 — warns once
    const packetGen0 = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([5]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([99]),
      material_handles: new Uint32Array([88]),
    });

    cache.applyFrame(packetGen0);
    expect(warnSpy).toHaveBeenCalledTimes(2);
    const oldObj = cache.getObject(5, 0)!;
    expect(scene.children).toContain(oldObj);

    // Gen 1: same slot reused — stale eviction must fire, fresh object created,
    // warning state reset so new unregistered handles warn again.
    warnSpy.mockClear();
    const packetGen1 = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([5]),
      entity_generations: new Uint32Array([1]),
      mesh_handles: new Uint32Array([77]),
      material_handles: new Uint32Array([66]),
    });

    cache.applyFrame(packetGen1);

    // Old object removed from scene
    expect(scene.children).not.toContain(oldObj);
    // Old gen lookup returns undefined (generational safety)
    expect(cache.getObject(5, 0)).toBeUndefined();
    // New object exists under new generation
    const newObj = cache.getObject(5, 1)!;
    expect(newObj).toBeDefined();
    expect(newObj).not.toBe(oldObj);
    expect(scene.children).toContain(newObj);
    // Warnings fired again for the new (also unregistered) handles
    expect(warnSpy).toHaveBeenCalledTimes(2);
    expect(cache.objectCount).toBe(1);

    warnSpy.mockRestore();
  });

  // ---------------------------------------------------------------------------
  // T3-G: Late registration upgrades placeholder to real asset
  // ---------------------------------------------------------------------------

  test("late registration upgrades entity from placeholder to real asset", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const warnSpy = spyOn(console, "warn").mockImplementation(() => {});

    // Frame 1: handle 99 not yet registered — gets placeholder
    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([99]),
      material_handles: new Uint32Array([99]),
    });

    cache.applyFrame(packet);
    const obj = cache.getObject(1, 0)!;
    expect(warnSpy).toHaveBeenCalledTimes(2);

    // Register the assets after rendering started
    const lateGeo = new THREE.SphereGeometry(2);
    const lateMat = new THREE.MeshBasicMaterial({ color: 0x00ff00 });
    cache.registerGeometry(99, lateGeo);
    cache.registerMaterial(99, lateMat);

    // Frame 2: same handle 99 — should upgrade from placeholder to real asset
    cache.applyFrame(packet);
    expect(obj.geometry).toBe(lateGeo);
    expect(obj.material).toBe(lateMat);

    // Frame 3: stable — no further assignment
    obj.material = new THREE.MeshBasicMaterial({ color: 0xff0000 }); // consumer override
    cache.applyFrame(packet);
    expect(obj.material).not.toBe(lateMat); // consumer override survives now

    warnSpy.mockRestore();
  });

  // ---------------------------------------------------------------------------
  // T3-H: Same-handle rebind updates entity to new asset
  // ---------------------------------------------------------------------------

  test("rebinding a registry entry under the same handle updates entities", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const matA = new THREE.MeshBasicMaterial({ color: 0xff0000 });
    const matB = new THREE.MeshBasicMaterial({ color: 0x0000ff });
    const geoA = new THREE.BoxGeometry();
    const geoB = new THREE.SphereGeometry(1);
    cache.registerGeometry(1, geoA);
    cache.registerMaterial(1, matA);

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([10]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    });

    cache.applyFrame(packet);
    const obj = cache.getObject(10, 0)!;
    expect(obj.geometry).toBe(geoA);
    expect(obj.material).toBe(matA);

    // Rebind handle 1 to new assets
    cache.registerGeometry(1, geoB);
    cache.registerMaterial(1, matB);

    // Frame 2: same handle, but registry entry changed — should update
    cache.applyFrame(packet);
    expect(obj.geometry).toBe(geoB);
    expect(obj.material).toBe(matB);
  });
});

// ---------------------------------------------------------------------------
// #133: matrixAutoUpdate disabled, matrix.compose() called explicitly
// ---------------------------------------------------------------------------

describe("RendererCache matrix management", () => {
  test("matrixAutoUpdate is false on created objects", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry());
    cache.registerMaterial(1, new THREE.MeshBasicMaterial());

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    });

    cache.applyFrame(packet);
    const obj = cache.getObject(1, 0)!;
    expect(obj.matrixAutoUpdate).toBe(false);
  });

  test("matrixWorldNeedsUpdate is true after applyFrame", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry());
    cache.registerMaterial(1, new THREE.MeshBasicMaterial());

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    });

    cache.applyFrame(packet);
    const obj = cache.getObject(1, 0)!;
    expect(obj.matrixWorldNeedsUpdate).toBe(true);
  });

  test("matrix elements match compose() output after applyFrame with known transform", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry());
    cache.registerMaterial(1, new THREE.MeshBasicMaterial());

    const transforms = new Float32Array(TRANSFORM_STRIDE);
    // position: (2, 3, 4)
    transforms[0] = 2; transforms[1] = 3; transforms[2] = 4;
    // quaternion (x, y, z, w): identity
    transforms[3] = 0; transforms[4] = 0; transforms[5] = 0; transforms[6] = 1;
    // scale: (1, 2, 0.5)
    transforms[7] = 1; transforms[8] = 2; transforms[9] = 0.5;

    const packet = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([2]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
      transforms,
    });

    cache.applyFrame(packet);
    const obj = cache.getObject(2, 0)!;

    const expected = new THREE.Matrix4();
    expected.compose(
      new THREE.Vector3(2, 3, 4),
      new THREE.Quaternion(0, 0, 0, 1),
      new THREE.Vector3(1, 2, 0.5),
    );

    expect(obj.matrix.elements).toEqual(expected.elements);
  });
});
