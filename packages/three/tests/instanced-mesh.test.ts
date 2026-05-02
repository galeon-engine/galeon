// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  CHANGED_INSTANCE_GROUP,
  CHANGED_TRANSFORM,
  INSTANCE_GROUP_NONE,
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "../../render-core/src/index.js";
import { RendererCache } from "../src/renderer-cache.js";

interface PacketOverrides extends Partial<FramePacketView> {
  entity_count: number;
}

function makePacket(overrides: PacketOverrides): FramePacketView {
  const count = overrides.entity_count;
  return {
    contract_version: RENDER_CONTRACT_VERSION,
    entity_count: count,
    entity_ids: new Uint32Array(count),
    entity_generations: new Uint32Array(count),
    transforms: new Float32Array(count * TRANSFORM_STRIDE),
    visibility: new Uint8Array(count).fill(1),
    mesh_handles: new Uint32Array(count),
    material_handles: new Uint32Array(count),
    parent_ids: new Uint32Array(count).fill(SCENE_ROOT),
    custom_channel_count: 0,
    custom_channel_name_at: () => "",
    custom_channel_stride: () => 1,
    custom_channel_data: () => new Float32Array(0),
    event_count: 0,
    event_kinds: new Uint32Array(0),
    event_entities: new Uint32Array(0),
    event_positions: new Float32Array(0),
    event_intensities: new Float32Array(0),
    event_data: new Float32Array(0),
    ...overrides,
  };
}

function fillIdentityTransforms(packet: FramePacketView): void {
  for (let i = 0; i < packet.entity_count; i++) {
    const off = i * TRANSFORM_STRIDE;
    // position (0,0,0); quaternion (0,0,0,1); scale (1,1,1)
    packet.transforms[off + 6] = 1;
    packet.transforms[off + 7] = 1;
    packet.transforms[off + 8] = 1;
    packet.transforms[off + 9] = 1;
  }
}

describe("RendererCache instanced-mesh path (#215 T2)", () => {
  test("1000 tagged entities produce 1 InstancedMesh with count >= 1000", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const N = 1000;
    const packet = makePacket({ entity_count: N });
    fillIdentityTransforms(packet);
    for (let i = 0; i < N; i++) {
      packet.entity_ids[i] = i + 1; // avoid 0 to make sentinel collisions visible
      packet.mesh_handles[i] = 7;
    }
    const groups = new Uint32Array(N);
    groups.fill(7);
    (packet as { instance_groups?: Uint32Array }).instance_groups = groups;

    cache.applyFrame(packet);

    const mesh = cache.instancing.meshFor(7);
    expect(mesh).toBeDefined();
    expect(mesh!.count).toBeGreaterThanOrEqual(N);
    expect(cache.instancing.batchCount).toBe(1);
    // Standalone path is untouched for tagged entities.
    expect(cache.objectCount).toBe(0);
    // Mesh must actually be in the scene graph.
    expect(mesh!.parent).toBe(scene);
  });

  test("capacity grows by 2x when slot count exceeds initial capacity", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(3, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    // Two entities trigger lazy create at INITIAL_CAPACITY=16.
    const small = makePacket({ entity_count: 2 });
    fillIdentityTransforms(small);
    for (let i = 0; i < 2; i++) {
      small.entity_ids[i] = i + 1;
      small.mesh_handles[i] = 3;
    }
    (small as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([3, 3]);
    cache.applyFrame(small);
    expect(cache.instancing.capacityFor(3)).toBe(16);

    // Bump to 17 entities — capacity must grow to at least 32 (2x).
    const N = 17;
    const big = makePacket({ entity_count: N });
    fillIdentityTransforms(big);
    for (let i = 0; i < N; i++) {
      big.entity_ids[i] = i + 1;
      big.mesh_handles[i] = 3;
    }
    const groups = new Uint32Array(N);
    groups.fill(3);
    (big as { instance_groups?: Uint32Array }).instance_groups = groups;
    cache.applyFrame(big);

    expect(cache.instancing.capacityFor(3)).toBeGreaterThanOrEqual(32);
    expect(cache.instancing.slotsFor(3)).toBe(N);
  });

  test("removed instanced entity frees its slot via swap-with-last", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(5, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const N = 4;
    const packet = makePacket({ entity_count: N });
    fillIdentityTransforms(packet);
    for (let i = 0; i < N; i++) {
      packet.entity_ids[i] = i + 1;
      packet.mesh_handles[i] = 5;
    }
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([5, 5, 5, 5]);
    cache.applyFrame(packet);
    expect(cache.instancing.slotsFor(5)).toBe(4);

    // Drop the middle two — full packet, the rest must persist.
    const next = makePacket({ entity_count: 2 });
    fillIdentityTransforms(next);
    next.entity_ids[0] = 1;
    next.entity_ids[1] = 4;
    next.mesh_handles[0] = 5;
    next.mesh_handles[1] = 5;
    (next as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([5, 5]);
    cache.applyFrame(next);

    expect(cache.instancing.slotsFor(5)).toBe(2);
    expect(cache.instancing.meshFor(5)!.count).toBe(2);
  });

  test("CHANGED_INSTANCE_GROUP migrates entity from instanced to standalone", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(2, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    // Frame 1: one instanced entity.
    const f1 = makePacket({ entity_count: 1 });
    fillIdentityTransforms(f1);
    f1.entity_ids[0] = 42;
    f1.mesh_handles[0] = 2;
    (f1 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([2]);
    cache.applyFrame(f1);
    expect(cache.instancing.has(42)).toBe(true);
    expect(cache.objectCount).toBe(0);

    // Frame 2 (incremental): same entity, now standalone (group=NONE),
    // change_flags signal the migration.
    const f2 = makePacket({ entity_count: 1 });
    fillIdentityTransforms(f2);
    f2.entity_ids[0] = 42;
    f2.mesh_handles[0] = 2;
    (f2 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([INSTANCE_GROUP_NONE]);
    (f2 as { change_flags?: Uint8Array }).change_flags = new Uint8Array([
      CHANGED_INSTANCE_GROUP | CHANGED_TRANSFORM,
    ]);
    cache.applyFrame(f2);

    expect(cache.instancing.has(42)).toBe(false);
    expect(cache.instancing.slotsFor(2)).toBe(0);
    expect(cache.objectCount).toBe(1);
  });

  test("instanced to standalone keeps packet parent without CHANGED_PARENT", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(2, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    // Frame 1: parent is standalone, child is instanced under that parent.
    const f1 = makePacket({ entity_count: 2 });
    fillIdentityTransforms(f1);
    f1.entity_ids[0] = 100;
    f1.entity_ids[1] = 42;
    f1.mesh_handles[0] = 2;
    f1.mesh_handles[1] = 2;
    f1.parent_ids[1] = 100;
    (f1 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([INSTANCE_GROUP_NONE, 2]);
    cache.applyFrame(f1);

    expect(cache.instancing.has(42)).toBe(true);
    const parentObj = cache.getObject(100, 0);
    expect(parentObj).toBeDefined();

    // Frame 2 (incremental): child leaves instancing, parent stays unchanged.
    const f2 = makePacket({ entity_count: 1 });
    fillIdentityTransforms(f2);
    f2.entity_ids[0] = 42;
    f2.mesh_handles[0] = 2;
    f2.parent_ids[0] = 100;
    (f2 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([INSTANCE_GROUP_NONE]);
    (f2 as { change_flags?: Uint8Array }).change_flags = new Uint8Array([
      CHANGED_INSTANCE_GROUP | CHANGED_TRANSFORM,
    ]);
    cache.applyFrame(f2);

    const childObj = cache.getObject(42, 0);
    expect(childObj).toBeDefined();
    expect(childObj!.parent).toBe(parentObj!);
  });

  test("CHANGED_INSTANCE_GROUP migrates entity between batches", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(10, new THREE.BoxGeometry(1, 1, 1));
    cache.registerGeometry(11, new THREE.SphereGeometry(1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const f1 = makePacket({ entity_count: 1 });
    fillIdentityTransforms(f1);
    f1.entity_ids[0] = 7;
    f1.mesh_handles[0] = 10;
    (f1 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([10]);
    cache.applyFrame(f1);
    expect(cache.instancing.slotsFor(10)).toBe(1);
    expect(cache.instancing.slotsFor(11)).toBe(0);

    const f2 = makePacket({ entity_count: 1 });
    fillIdentityTransforms(f2);
    f2.entity_ids[0] = 7;
    f2.mesh_handles[0] = 11;
    (f2 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([11]);
    (f2 as { change_flags?: Uint8Array }).change_flags = new Uint8Array([
      CHANGED_INSTANCE_GROUP,
    ]);
    cache.applyFrame(f2);

    expect(cache.instancing.slotsFor(10)).toBe(0);
    expect(cache.instancing.slotsFor(11)).toBe(1);
  });

  test("instance matrix carries position from transforms array", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 1;
    packet.mesh_handles[0] = 1;
    packet.transforms[0] = 5; // position.x
    packet.transforms[1] = 7; // position.y
    packet.transforms[2] = 9; // position.z
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1]);
    cache.applyFrame(packet);

    const mesh = cache.instancing.meshFor(1)!;
    const m = new THREE.Matrix4();
    mesh.getMatrixAt(0, m);
    const pos = new THREE.Vector3();
    pos.setFromMatrixPosition(m);
    expect(pos.x).toBeCloseTo(5);
    expect(pos.y).toBeCloseTo(7);
    expect(pos.z).toBeCloseTo(9);
  });

  test("existing batches rebind when handle registrations change", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 1;
    packet.mesh_handles[0] = 1;
    packet.material_handles[0] = 2;
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1]);

    cache.applyFrame(packet);
    const mesh = cache.instancing.meshFor(1)!;
    const placeholderGeometry = mesh.geometry;
    const placeholderMaterial = mesh.material;

    const registeredGeometry = new THREE.SphereGeometry(1);
    const registeredMaterial = new THREE.MeshBasicMaterial({ color: 0x00ff00 });
    cache.registerGeometry(1, registeredGeometry);
    cache.registerMaterial(2, registeredMaterial);
    cache.applyFrame(packet);

    expect(cache.instancing.meshFor(1)).toBe(mesh);
    expect(mesh.geometry).toBe(registeredGeometry);
    expect(mesh.material).toBe(registeredMaterial);
    expect(mesh.geometry).not.toBe(placeholderGeometry);
    expect(mesh.material).not.toBe(placeholderMaterial);
  });

  test("hidden instance writes zero scale", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 1;
    packet.mesh_handles[0] = 1;
    packet.visibility[0] = 0;
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1]);
    cache.applyFrame(packet);

    const mesh = cache.instancing.meshFor(1)!;
    const m = new THREE.Matrix4();
    mesh.getMatrixAt(0, m);
    const scale = new THREE.Vector3();
    scale.setFromMatrixScale(m);
    expect(scale.x).toBeCloseTo(0);
    expect(scale.y).toBeCloseTo(0);
    expect(scale.z).toBeCloseTo(0);
  });

  test("dispose() detaches batches from the scene", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 3 });
    fillIdentityTransforms(packet);
    for (let i = 0; i < 3; i++) {
      packet.entity_ids[i] = i + 1;
      packet.mesh_handles[i] = 1;
    }
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1, 1, 1]);
    cache.applyFrame(packet);
    expect(scene.children.length).toBeGreaterThan(0);

    cache.dispose();
    expect(scene.children.length).toBe(0);
    expect(cache.instancing.batchCount).toBe(0);
  });
});

describe("RendererCache instanced tint channel (#215 T3)", () => {
  test("tinted entity round-trips into InstancedMesh.instanceColor", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 1;
    packet.mesh_handles[0] = 1;
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1]);
    (packet as { tints?: Float32Array }).tints = new Float32Array([
      0.25, 0.5, 1.0,
    ]);
    cache.applyFrame(packet);

    const mesh = cache.instancing.meshFor(1)!;
    expect(mesh.instanceColor).toBeDefined();
    const color = new THREE.Color();
    mesh.getColorAt(0, color);
    expect(color.r).toBeCloseTo(0.25);
    expect(color.g).toBeCloseTo(0.5);
    expect(color.b).toBeCloseTo(1.0);
  });

  test("untinted entities default to white when packet has no tints", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 1;
    packet.mesh_handles[0] = 1;
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1]);
    cache.applyFrame(packet);

    const mesh = cache.instancing.meshFor(1)!;
    // instanceColor is allocated synchronously at batch creation per #21786,
    // even when no tint arrives — every slot defaults to white.
    expect(mesh.instanceColor).toBeDefined();
    const color = new THREE.Color();
    mesh.getColorAt(0, color);
    expect(color.r).toBeCloseTo(1.0);
    expect(color.g).toBeCloseTo(1.0);
    expect(color.b).toBeCloseTo(1.0);
  });

  test("mixed tinted + untinted entities in same batch", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 3 });
    fillIdentityTransforms(packet);
    for (let i = 0; i < 3; i++) {
      packet.entity_ids[i] = i + 1;
      packet.mesh_handles[i] = 1;
    }
    (packet as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1, 1, 1]);
    // Slot 0 red, slot 1 white (un-tinted), slot 2 green.
    (packet as { tints?: Float32Array }).tints = new Float32Array([
      1, 0, 0, 1, 1, 1, 0, 1, 0,
    ]);
    cache.applyFrame(packet);

    const mesh = cache.instancing.meshFor(1)!;
    const color = new THREE.Color();
    mesh.getColorAt(0, color);
    expect([color.r, color.g, color.b]).toEqual([1, 0, 0]);
    mesh.getColorAt(1, color);
    expect([color.r, color.g, color.b]).toEqual([1, 1, 1]);
    mesh.getColorAt(2, color);
    expect([color.r, color.g, color.b]).toEqual([0, 1, 0]);
  });

  test("growBatch carries instanceColor through 2x grow", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    // Frame 1: 2 entities at INITIAL_CAPACITY=16, slot 0 = red.
    const f1 = makePacket({ entity_count: 2 });
    fillIdentityTransforms(f1);
    for (let i = 0; i < 2; i++) {
      f1.entity_ids[i] = i + 1;
      f1.mesh_handles[i] = 1;
    }
    (f1 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1, 1]);
    (f1 as { tints?: Float32Array }).tints = new Float32Array([
      1, 0, 0, 1, 1, 1,
    ]);
    cache.applyFrame(f1);
    expect(cache.instancing.capacityFor(1)).toBe(16);

    // Frame 2: 17 entities forces grow. Slot 0 still red.
    const N = 17;
    const f2 = makePacket({ entity_count: N });
    fillIdentityTransforms(f2);
    for (let i = 0; i < N; i++) {
      f2.entity_ids[i] = i + 1;
      f2.mesh_handles[i] = 1;
    }
    const groups = new Uint32Array(N);
    groups.fill(1);
    (f2 as { instance_groups?: Uint32Array }).instance_groups = groups;
    const tints = new Float32Array(N * 3);
    for (let i = 0; i < N; i++) {
      tints[i * 3] = i === 0 ? 1 : 1;
      tints[i * 3 + 1] = i === 0 ? 0 : 1;
      tints[i * 3 + 2] = i === 0 ? 0 : 1;
    }
    (f2 as { tints?: Float32Array }).tints = tints;
    cache.applyFrame(f2);

    expect(cache.instancing.capacityFor(1)).toBeGreaterThanOrEqual(32);
    const mesh = cache.instancing.meshFor(1)!;
    expect(mesh.instanceColor).toBeDefined();
    const color = new THREE.Color();
    mesh.getColorAt(0, color);
    expect([color.r, color.g, color.b]).toEqual([1, 0, 0]);
  });

  test("swap-with-last on remove preserves moved entity's tint", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    // Frame 1: 4 entities, distinctive tints per slot.
    const f1 = makePacket({ entity_count: 4 });
    fillIdentityTransforms(f1);
    for (let i = 0; i < 4; i++) {
      f1.entity_ids[i] = i + 1;
      f1.mesh_handles[i] = 1;
    }
    (f1 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1, 1, 1, 1]);
    // entity 1 = red, entity 2 = green, entity 3 = blue, entity 4 = yellow.
    (f1 as { tints?: Float32Array }).tints = new Float32Array([
      1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0,
    ]);
    cache.applyFrame(f1);

    // Frame 2: drop entities 2 and 3, keep 1 and 4.
    const f2 = makePacket({ entity_count: 2 });
    fillIdentityTransforms(f2);
    f2.entity_ids[0] = 1;
    f2.entity_ids[1] = 4;
    f2.mesh_handles[0] = 1;
    f2.mesh_handles[1] = 1;
    (f2 as { instance_groups?: Uint32Array }).instance_groups =
      new Uint32Array([1, 1]);
    (f2 as { tints?: Float32Array }).tints = new Float32Array([
      1, 0, 0, 1, 1, 0,
    ]);
    cache.applyFrame(f2);

    expect(cache.instancing.slotsFor(1)).toBe(2);
    const mesh = cache.instancing.meshFor(1)!;
    const color = new THREE.Color();
    // Both surviving entities have their tints intact regardless of slot.
    const observed: Array<[number, number, number]> = [];
    for (let s = 0; s < mesh.count; s++) {
      mesh.getColorAt(s, color);
      observed.push([color.r, color.g, color.b]);
    }
    expect(observed).toContainEqual([1, 0, 0]);
    expect(observed).toContainEqual([1, 1, 0]);
  });
});
