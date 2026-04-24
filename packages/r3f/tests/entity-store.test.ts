// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  CHANGED_TRANSFORM,
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "../../render-core/src/index.js";
import { RendererCache } from "../../three/src/renderer-cache.js";
import { GaleonEntityStore } from "../src/entity-store.js";

function makeTransforms(entityCount: number): Float32Array {
  const transforms = new Float32Array(entityCount * TRANSFORM_STRIDE);
  Array.from({ length: entityCount }, (_, i) => i).forEach((i) => {
    const offset = i * TRANSFORM_STRIDE;
    transforms[offset + 6] = 1;
    transforms[offset + 7] = 1;
    transforms[offset + 8] = 1;
    transforms[offset + 9] = 1;
  });
  return transforms;
}

function makePacket(
  overrides: Partial<FramePacketView> & { entity_count: number },
): FramePacketView {
  const count = overrides.entity_count;
  return {
    contract_version: RENDER_CONTRACT_VERSION,
    entity_count: count,
    entity_ids: new Uint32Array(count),
    entity_generations: new Uint32Array(count),
    transforms: makeTransforms(count),
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

describe("GaleonEntityStore hot-update behavior", () => {
  test("transform-only full-frame updates keep entity refs and object identity stable", () => {
    const cache = new RendererCache(new THREE.Scene());
    const store = new GaleonEntityStore();

    const packetA = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      frame_version: 1n,
    });
    packetA.transforms[0] = 1;
    cache.applyFrame(packetA);
    expect(store.sync(packetA, cache)).toBe(true);

    const refA = store.get(1, 0)!;
    const objectA = refA.object;
    expect(objectA).toBeDefined();

    const packetB = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      frame_version: 2n,
    });
    packetB.transforms[0] = 9;
    cache.applyFrame(packetB);
    expect(store.sync(packetB, cache)).toBe(false);

    const refB = store.get(1, 0)!;
    expect(refB).toBe(refA);
    expect(refB.object).toBe(objectA);
    expect(refB.transform[0]).toBe(9);
  });

  test("incremental packets update hot entities without dropping untouched refs", () => {
    const cache = new RendererCache(new THREE.Scene());
    const store = new GaleonEntityStore();

    const fullFrame = makePacket({
      entity_count: 2,
      entity_ids: new Uint32Array([1, 2]),
      entity_generations: new Uint32Array([0, 0]),
      frame_version: 1n,
    });
    cache.applyFrame(fullFrame);
    expect(store.sync(fullFrame, cache)).toBe(true);

    const coldRefBefore = store.get(2, 0)!;
    const coldObjectBefore = coldRefBefore.object;

    const hotUpdate = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      frame_version: 2n,
      change_flags: new Uint8Array([CHANGED_TRANSFORM]),
    });
    hotUpdate.transforms[0] = 77;
    cache.applyFrame(hotUpdate);
    expect(store.sync(hotUpdate, cache)).toBe(false);

    const hotRef = store.get(1, 0)!;
    const coldRefAfter = store.get(2, 0)!;
    expect(hotRef.transform[0]).toBe(77);
    expect(coldRefAfter).toBe(coldRefBefore);
    expect(coldRefAfter.object).toBe(coldObjectBefore);
  });

  test("incremental structural additions publish a new entities array", () => {
    const cache = new RendererCache(new THREE.Scene());
    const store = new GaleonEntityStore();

    const fullFrame = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      frame_version: 1n,
    });
    cache.applyFrame(fullFrame);
    expect(store.sync(fullFrame, cache)).toBe(true);
    const entitiesBefore = store.entities();

    const incrementalSpawn = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([2]),
      entity_generations: new Uint32Array([0]),
      frame_version: 2n,
      change_flags: new Uint8Array([CHANGED_TRANSFORM]),
    });
    cache.applyFrame(incrementalSpawn);
    expect(store.sync(incrementalSpawn, cache)).toBe(true);

    expect(store.entities()).not.toBe(entitiesBefore);
    expect(store.entities().map((entity) => entity.entityId)).toEqual([1, 2]);
  });

  test("generation reuse is structural and replaces the stable entity ref", () => {
    const cache = new RendererCache(new THREE.Scene());
    const store = new GaleonEntityStore();

    const first = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([5]),
      entity_generations: new Uint32Array([0]),
      frame_version: 1n,
    });
    cache.applyFrame(first);
    expect(store.sync(first, cache)).toBe(true);
    const oldRef = store.get(5, 0)!;

    const reused = makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([5]),
      entity_generations: new Uint32Array([1]),
      frame_version: 2n,
    });
    cache.applyFrame(reused);
    expect(store.sync(reused, cache)).toBe(true);

    expect(store.get(5, 0)).toBeUndefined();
    expect(store.get(5, 1)).not.toBe(oldRef);
  });
});
