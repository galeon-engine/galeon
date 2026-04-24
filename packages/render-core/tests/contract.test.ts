// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import {
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  assertFramePacketContract,
  type FramePacketView,
} from "../src/index.js";

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

describe("render-core contract checks", () => {
  test("accepts well-formed packet", () => {
    const packet = makePacket({ entity_count: 2 });
    packet.transforms[6] = 1;
    packet.transforms[7] = 1;
    packet.transforms[8] = 1;
    packet.transforms[9] = 1;

    expect(() => assertFramePacketContract(packet)).not.toThrow();
  });

  test("rejects malformed transform table", () => {
    const packet = makePacket({
      entity_count: 2,
      transforms: new Float32Array(1),
    });

    expect(() => assertFramePacketContract(packet)).toThrow("transforms");
  });

  test("strict mode rejects packets without contract_version", () => {
    const packet = makePacket({
      entity_count: 0,
      contract_version: undefined,
    });

    expect(() =>
      assertFramePacketContract(packet, {
        allowMissingContractVersion: false,
      }),
    ).toThrow("missing contract_version");
  });
});
