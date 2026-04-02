// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
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
