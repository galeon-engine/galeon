// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import {
  CHANGED_MATERIAL,
  CHANGED_TRANSFORM,
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  TransformFrameIngestion,
  frameEntityKey,
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

function setTransform(packet: FramePacketView, index: number, x: number): void {
  const offset = index * TRANSFORM_STRIDE;
  packet.transforms[offset] = x;
  packet.transforms[offset + 1] = 0;
  packet.transforms[offset + 2] = 0;
  packet.transforms[offset + 3] = 0;
  packet.transforms[offset + 4] = 0;
  packet.transforms[offset + 5] = 0;
  packet.transforms[offset + 6] = 1;
  packet.transforms[offset + 7] = 1;
  packet.transforms[offset + 8] = 1;
  packet.transforms[offset + 9] = 1;
}

describe("TransformFrameIngestion", () => {
  test("samples transforms between authoritative frames", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });
    const first = makePacket({ entity_count: 1 });
    first.entity_ids[0] = 7;
    first.entity_generations[0] = 2;
    setTransform(first, 0, 0);

    const second = makePacket({ entity_count: 1 });
    second.entity_ids[0] = 7;
    second.entity_generations[0] = 2;
    setTransform(second, 0, 10);

    ingestion.ingestFrame(first, 0);
    ingestion.ingestFrame(second, 100);

    expect(ingestion.sampleEntity(7, 2, 50)?.x).toBe(5);
    expect(ingestion.sampleFrame(50)).toEqual([
      {
        key: frameEntityKey(7, 2),
        entityId: 7,
        generation: 2,
        visible: true,
        transform: {
          x: 5,
          y: 0,
          z: 0,
          qx: 0,
          qy: 0,
          qz: 0,
          qw: 1,
          sx: 1,
          sy: 1,
          sz: 1,
        },
      },
    ]);
  });

  test("full frames evict missing entities but incremental frames do not", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });
    const full = makePacket({ entity_count: 1 });
    full.entity_ids[0] = 1;
    setTransform(full, 0, 3);
    ingestion.ingestFrame(full, 0);

    const incremental = makePacket({
      entity_count: 1,
      change_flags: new Uint8Array([CHANGED_MATERIAL]),
    });
    incremental.entity_ids[0] = 2;
    setTransform(incremental, 0, 4);
    ingestion.ingestFrame(incremental, 16);

    expect(ingestion.sampleEntity(1, 0, 16)?.x).toBe(3);

    const emptyFull = makePacket({ entity_count: 0 });
    ingestion.ingestFrame(emptyFull, 32);

    expect(ingestion.sampleEntity(1, 0, 32)).toBeUndefined();
  });

  test("incremental transform changes add new samples", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });
    const first = makePacket({ entity_count: 1 });
    first.entity_ids[0] = 9;
    setTransform(first, 0, 0);
    ingestion.ingestFrame(first, 0);

    const incremental = makePacket({
      entity_count: 1,
      change_flags: new Uint8Array([CHANGED_TRANSFORM]),
    });
    incremental.entity_ids[0] = 9;
    setTransform(incremental, 0, 20);
    ingestion.ingestFrame(incremental, 100);

    expect(ingestion.sampleEntity(9, 0, 25)?.x).toBe(5);
  });
});
