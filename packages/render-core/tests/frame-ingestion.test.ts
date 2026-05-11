// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import {
  CHANGED_MATERIAL,
  CHANGED_TRANSFORM,
  CHANGED_VISIBILITY,
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  StateInterpolationBuffer,
  TRANSFORM_STRIDE,
  TransformFrameIngestion,
  assertFramePacketContract,
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
  test("StateInterpolationBuffer keeps history bounded while preserving two samples", () => {
    const buffer = new StateInterpolationBuffer<string, number>(
      {
        interpolate(from, to, alpha) {
          return from + (to - from) * alpha;
        },
      },
      { maxHistoryMs: 50 },
    );

    buffer.push("unit", 0, 0);
    buffer.push("unit", 100, 100);
    buffer.push("unit", 200, 200);

    expect(buffer.sample("unit", 0)).toBe(100);
    expect(buffer.sample("unit", 150)).toBe(150);
    expect(buffer.sample("unit", 200)).toBe(200);
  });

  test("StateInterpolationBuffer inserts out-of-order samples correctly", () => {
    const buffer = new StateInterpolationBuffer<string, number>(
      {
        interpolate(from, to, alpha) {
          return from + (to - from) * alpha;
        },
      },
      { maxHistoryMs: 1_000 },
    );

    buffer.push("unit", 100, 100);
    buffer.push("unit", 0, 0);
    buffer.push("unit", 50, 50);

    expect(buffer.sample("unit", 25)).toBe(25);
    expect(buffer.sample("unit", 75)).toBe(75);
  });

  test("StateInterpolationBuffer supports has/snapshot/delete/clear/keys", () => {
    const buffer = new StateInterpolationBuffer<string, number>(
      {
        interpolate(from, to, alpha) {
          return from + (to - from) * alpha;
        },
      },
      { maxHistoryMs: 1_000 },
    );

    buffer.push("a", 0, 0);
    buffer.push("a", 100, 100);
    buffer.push("b", 100, 200);

    expect(buffer.has("a")).toBe(true);
    expect(buffer.has("missing")).toBe(false);
    expect(Array.from(buffer.keys())).toEqual(["a", "b"]);

    const snapshot = buffer.snapshot(50);
    expect(snapshot.get("a")).toBe(50);
    expect(snapshot.get("b")).toBe(200);

    buffer.delete("a");
    expect(buffer.has("a")).toBe(false);
    expect(Array.from(buffer.keys())).toEqual(["b"]);

    buffer.clear();
    expect(Array.from(buffer.keys())).toEqual([]);
    expect(buffer.snapshot(100).size).toBe(0);
  });

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
    ingestion.ingestIncrementalFrame(incremental, 16);

    expect(ingestion.sampleEntity(1, 0, 16)?.x).toBe(3);

    const emptyFull = makePacket({ entity_count: 0 });
    ingestion.ingestFrame(emptyFull, 32);

    expect(ingestion.sampleEntity(1, 0, 32)).toBeUndefined();
  });

  test("empty incremental packets do not evict tracked entities", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const full = makePacket({ entity_count: 1 });
    full.entity_ids[0] = 3;
    setTransform(full, 0, 9);
    ingestion.ingestFrame(full, 0);

    const emptyIncremental = makePacket({
      entity_count: 0,
      change_flags: new Uint8Array(0),
    });
    ingestion.ingestIncrementalFrame(emptyIncremental, 16);

    expect(ingestion.sampleEntity(3, 0, 16)?.x).toBe(9);
  });

  test("full mode accepts empty change_flags with entities and evicts stale rows", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const baseline = makePacket({ entity_count: 2 });
    baseline.entity_ids[0] = 41;
    baseline.entity_ids[1] = 42;
    setTransform(baseline, 0, 1);
    setTransform(baseline, 1, 2);
    ingestion.ingestFrame(baseline, 0);

    const fullWithEmptyFlags = makePacket({
      entity_count: 1,
      change_flags: new Uint8Array(0),
    });
    fullWithEmptyFlags.entity_ids[0] = 42;
    setTransform(fullWithEmptyFlags, 0, 20);
    ingestion.ingestFrame(fullWithEmptyFlags, 16);

    expect(ingestion.sampleEntity(41, 0, 16)).toBeUndefined();
    expect(ingestion.sampleEntity(42, 0, 16)?.x).toBe(20);
  });

  test("incremental mode with entities requires per-row change flags", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const malformed = makePacket({
      entity_count: 1,
      change_flags: new Uint8Array(0),
    });
    malformed.entity_ids[0] = 8;
    setTransform(malformed, 0, 1);

    expect(() => assertFramePacketContract(malformed)).not.toThrow();
    expect(
      () => ingestion.ingestFrame(malformed, 0, { mode: "incremental" }),
    ).toThrow(
      /change_flags/i,
    );
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
    ingestion.ingestIncrementalFrame(incremental, 100);

    expect(ingestion.sampleEntity(9, 0, 25)?.x).toBe(5);
  });

  test("visibility toggles are reflected in sampled frames", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const full = makePacket({ entity_count: 1 });
    full.entity_ids[0] = 4;
    setTransform(full, 0, 1);
    ingestion.ingestFrame(full, 0);

    const incremental = makePacket({
      entity_count: 1,
      change_flags: new Uint8Array([CHANGED_VISIBILITY]),
    });
    incremental.entity_ids[0] = 4;
    incremental.visibility[0] = 0;
    setTransform(incremental, 0, 1);
    ingestion.ingestIncrementalFrame(incremental, 50);

    expect(ingestion.sampleFrame(50)[0]?.visible).toBe(false);
  });

  test("generation rollover evicts old generation keys on full frames", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const first = makePacket({ entity_count: 1 });
    first.entity_ids[0] = 11;
    first.entity_generations[0] = 1;
    setTransform(first, 0, 2);
    ingestion.ingestFrame(first, 0);

    const rollover = makePacket({ entity_count: 1 });
    rollover.entity_ids[0] = 11;
    rollover.entity_generations[0] = 2;
    setTransform(rollover, 0, 6);
    ingestion.ingestFrame(rollover, 16);

    expect(ingestion.sampleEntity(11, 1, 16)).toBeUndefined();
    expect(ingestion.sampleEntity(11, 2, 16)?.x).toBe(6);
  });

  test("generation rollover evicts old generation keys on incremental frames", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const first = makePacket({ entity_count: 1 });
    first.entity_ids[0] = 12;
    first.entity_generations[0] = 1;
    setTransform(first, 0, 2);
    ingestion.ingestFrame(first, 0);

    const rollover = makePacket({
      entity_count: 1,
      change_flags: new Uint8Array([CHANGED_TRANSFORM]),
    });
    rollover.entity_ids[0] = 12;
    rollover.entity_generations[0] = 2;
    setTransform(rollover, 0, 6);
    ingestion.ingestIncrementalFrame(rollover, 16);

    expect(ingestion.sampleEntity(12, 1, 16)).toBeUndefined();
    expect(ingestion.sampleEntity(12, 2, 16)?.x).toBe(6);
    expect(ingestion.sampleFrame(16).map((sample) => sample.key)).toEqual([
      frameEntityKey(12, 2),
    ]);
  });

  test("default interpolation delay samples now-100ms", () => {
    let nowMs = 0;
    const ingestion = new TransformFrameIngestion({
      now: () => nowMs,
    });

    const first = makePacket({ entity_count: 1 });
    first.entity_ids[0] = 13;
    setTransform(first, 0, 0);
    ingestion.ingestFrame(first, 0);

    const second = makePacket({ entity_count: 1 });
    second.entity_ids[0] = 13;
    setTransform(second, 0, 200);
    ingestion.ingestFrame(second, 200);

    nowMs = 250;
    expect(ingestion.sampleFrame()[0]?.transform.x).toBe(150);
  });

  test("quaternion interpolation flips sign to keep shortest path", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const first = makePacket({ entity_count: 1 });
    first.entity_ids[0] = 20;
    setTransform(first, 0, 0);
    ingestion.ingestFrame(first, 0);

    const second = makePacket({ entity_count: 1 });
    second.entity_ids[0] = 20;
    setTransform(second, 0, 0);
    second.transforms[6] = -1;
    ingestion.ingestFrame(second, 100);

    expect(ingestion.sampleEntity(20, 0, 50)?.qw).toBe(1);
  });

  test("malformed transforms throw and do not partially mutate state", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const baseline = makePacket({ entity_count: 1 });
    baseline.entity_ids[0] = 30;
    baseline.visibility[0] = 1;
    setTransform(baseline, 0, 10);
    ingestion.ingestFrame(baseline, 0);

    const invalid = makePacket({
      entity_count: 2,
      change_flags: new Uint8Array([CHANGED_TRANSFORM, CHANGED_TRANSFORM]),
    });
    invalid.entity_ids[0] = 30;
    invalid.visibility[0] = 0;
    setTransform(invalid, 0, 20);
    invalid.entity_ids[1] = 31;
    setTransform(invalid, 1, 99);
    invalid.transforms[TRANSFORM_STRIDE] = Number.NaN;

    expect(() => ingestion.ingestFrame(invalid, 16)).toThrow(
      /non-finite x/i,
    );

    expect(ingestion.sampleEntity(30, 0, 16)?.x).toBe(10);
    expect(ingestion.sampleEntity(31, 0, 16)).toBeUndefined();
    expect(
      ingestion.sampleFrame(16).find((sample) => sample.key === frameEntityKey(30, 0))
        ?.visible,
    ).toBe(true);
  });

  test("clear removes tracked entities and interpolation history", () => {
    const ingestion = new TransformFrameIngestion({
      now: () => 0,
      interpolationDelayMs: 0,
    });

    const full = makePacket({ entity_count: 1 });
    full.entity_ids[0] = 99;
    full.entity_generations[0] = 3;
    setTransform(full, 0, 42);
    ingestion.ingestFrame(full, 0);

    expect(ingestion.sampleEntity(99, 3, 0)?.x).toBe(42);
    expect(ingestion.sampleFrame(0)).toHaveLength(1);

    ingestion.clear();

    expect(ingestion.sampleEntity(99, 3, 0)).toBeUndefined();
    expect(ingestion.sampleFrame(0)).toEqual([]);
  });
});
