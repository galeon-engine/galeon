// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import { createElement } from "react";
import * as THREE from "three";
import { act, create } from "@react-three/test-renderer";
import {
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "../../render-core/src/index.js";
import {
  GaleonProvider,
  useGaleonEntities,
  type GaleonEntityRef,
} from "../src/index.js";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makeTransforms(x: number): Float32Array {
  const transforms = new Float32Array(TRANSFORM_STRIDE);
  transforms[0] = x;
  transforms[6] = 1;
  transforms[7] = 1;
  transforms[8] = 1;
  transforms[9] = 1;
  return transforms;
}

function makePacket(frameVersion: bigint, x: number): FramePacketView {
  return {
    contract_version: RENDER_CONTRACT_VERSION,
    entity_count: 1,
    entity_ids: new Uint32Array([1]),
    entity_generations: new Uint32Array([0]),
    transforms: makeTransforms(x),
    visibility: new Uint8Array([1]),
    mesh_handles: new Uint32Array([0]),
    material_handles: new Uint32Array([0]),
    parent_ids: new Uint32Array([SCENE_ROOT]),
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
    frame_version: frameVersion,
  };
}

function EntityProbe({
  snapshots,
}: {
  snapshots: Array<readonly GaleonEntityRef[]>;
}) {
  snapshots.push(useGaleonEntities());
  return null;
}

describe("GaleonProvider R3F integration", () => {
  test("advances an engine source through R3F frames without React rerenders for hot transforms", async () => {
    const snapshots: Array<readonly GaleonEntityRef[]> = [];
    const emitted: FramePacketView[] = [
      makePacket(1n, 1),
      makePacket(2n, 9),
    ];
    let tickCount = 0;
    let extractCount = 0;
    const engine = {
      tick() {
        tickCount += 1;
      },
      extract_frame() {
        return emitted[Math.min(extractCount++, emitted.length - 1)]!;
      },
    };

    const renderer = await create(
      createElement(
        GaleonProvider,
        { engine },
        createElement(EntityProbe, { snapshots }),
      ),
    );

    await act(async () => {
      await renderer.advanceFrames(1, 1 / 60);
    });

    const firstEntity = snapshots.at(-1)?.[0];
    expect(firstEntity).toBeDefined();
    const object = firstEntity!.object;
    expect(object).toBeInstanceOf(THREE.Object3D);
    expect(object!.position.x).toBe(1);
    const renderCountAfterStructure = snapshots.length;

    await act(async () => {
      await renderer.advanceFrames(1, 1 / 60);
    });

    expect(tickCount).toBeGreaterThanOrEqual(2);
    expect(extractCount).toBeGreaterThanOrEqual(2);
    expect(snapshots).toHaveLength(renderCountAfterStructure);
    expect(firstEntity!.object).toBe(object);
    expect(object!.position.x).toBe(9);

    await renderer.unmount();
  });
});
