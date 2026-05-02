// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import { Fragment, createElement } from "react";
import { useThree } from "@react-three/fiber";
import { act, create } from "@react-three/test-renderer";
import * as THREE from "three";
import {
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "../../render-core/src/index.js";
import {
  GaleonProvider,
  MarqueeRenderer,
  SelectionRings,
} from "../src/index.js";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makePacket(): FramePacketView {
  const transforms = new Float32Array(TRANSFORM_STRIDE);
  transforms[6] = 1;
  transforms[7] = 1;
  transforms[8] = 1;
  transforms[9] = 1;
  return {
    contract_version: RENDER_CONTRACT_VERSION,
    entity_count: 1,
    entity_ids: new Uint32Array([1]),
    entity_generations: new Uint32Array([0]),
    transforms,
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
    frame_version: 1n,
  };
}

function findRingGroup(scene: THREE.Scene): THREE.Group | undefined {
  return scene.children.find((child) => child.name === "GaleonSelectionRings") as
    | THREE.Group
    | undefined;
}

function findMarqueeLine(camera: THREE.Camera): THREE.LineLoop | undefined {
  return camera.children.find((child) => child.name === "GaleonMarqueeRenderer") as
    | THREE.LineLoop
    | undefined;
}

let capturedScene: THREE.Scene | null = null;
let capturedCamera: THREE.Camera | null = null;

function SceneProbe() {
  capturedScene = useThree((state) => state.scene);
  capturedCamera = useThree((state) => state.camera);
  return null;
}

describe("selection HUD R3F bindings", () => {
  test("MarqueeRenderer mounts camera-attached geometry and follows rect updates", async () => {
    capturedCamera = null;
    const renderer = await create(
      createElement(
        Fragment,
        null,
        createElement(MarqueeRenderer, { rect: null }),
        createElement(SceneProbe),
      ),
    );

    await act(async () => {
      await renderer.advanceFrames(1, 1 / 60);
    });
    const line = findMarqueeLine(capturedCamera!)!;
    expect(line).toBeDefined();
    expect(line.visible).toBe(false);

    await act(async () => {
      await renderer.update(
        createElement(
          Fragment,
          null,
          createElement(MarqueeRenderer, {
            rect: { start: { x: -0.5, y: -0.25 }, end: { x: 0.5, y: 0.75 } },
          }),
          createElement(SceneProbe),
        ),
      );
      await renderer.advanceFrames(1, 1 / 60);
    });

    expect(line.visible).toBe(true);
    const positions = line.geometry.getAttribute("position");
    expect([positions.getX(0), positions.getY(0)]).toEqual([-0.5, -0.25]);
    expect([positions.getX(2), positions.getY(2)]).toEqual([0.5, 0.75]);

    await act(async () => {
      await renderer.unmount();
    });
    expect(capturedCamera!.children.some((child) => child.name === "GaleonMarqueeRenderer")).toBe(false);
  });

  test("SelectionRings updates ring structure when selection changes", async () => {
    capturedScene = null;
    const frame = makePacket();
    const renderer = await create(
      createElement(
        GaleonProvider,
        { frame },
        createElement(
          Fragment,
          null,
          createElement(SelectionRings, { selection: [] }),
          createElement(SceneProbe),
        ),
      ),
    );

    await act(async () => {
      await renderer.advanceFrames(1, 1 / 60);
    });
    expect(findRingGroup(capturedScene!)?.children).toHaveLength(0);

    await act(async () => {
      await renderer.update(
        createElement(
          GaleonProvider,
          { frame },
          createElement(
            Fragment,
            null,
            createElement(SelectionRings, { selection: [{ entityId: 1, generation: 0 }] }),
            createElement(SceneProbe),
          ),
        ),
      );
      await renderer.advanceFrames(1, 1 / 60);
    });
    expect(findRingGroup(capturedScene!)?.children).toHaveLength(1);

    await act(async () => {
      await renderer.update(
        createElement(
          GaleonProvider,
          { frame },
          createElement(
            Fragment,
            null,
            createElement(SelectionRings, { selection: [] }),
            createElement(SceneProbe),
          ),
        ),
      );
      await renderer.advanceFrames(1, 1 / 60);
    });
    expect(findRingGroup(capturedScene!)?.children).toHaveLength(0);

    await act(async () => {
      await renderer.unmount();
    });
    expect(capturedScene!.children.some((child) => child.name === "GaleonSelectionRings")).toBe(false);
  });
});
