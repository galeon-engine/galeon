// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "../../render-core/src/index.js";
import { RendererCache } from "../src/renderer-cache.js";

function makePacket(
  overrides: Partial<FramePacketView> & { entity_count: number },
): FramePacketView {
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

describe("RendererCache render contract guardrails", () => {
  test("throws on contract version mismatch", () => {
    const cache = new RendererCache(new THREE.Scene());
    const packet = makePacket({
      entity_count: 0,
      contract_version: RENDER_CONTRACT_VERSION + 1,
    });

    expect(() => cache.applyFrame(packet)).toThrow("contract_version");
  });

  test("allows legacy packets with missing contract_version", () => {
    const cache = new RendererCache(new THREE.Scene());
    const packet = makePacket({
      entity_count: 1,
      contract_version: undefined,
    });
    packet.transforms[6] = 1;
    packet.transforms[7] = 1;
    packet.transforms[8] = 1;
    packet.transforms[9] = 1;

    expect(() => cache.applyFrame(packet)).not.toThrow();
  });
});
