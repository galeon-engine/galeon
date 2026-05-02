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
  MarqueeOverlay,
  SelectionRings,
} from "../src/index.js";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface FakeListener {
  type: string;
  fn: (event: MouseEvent) => void;
}

class FakeStyle {
  position = "";
  pointerEvents = "";
  boxSizing = "";
  border = "";
  background = "";
  zIndex = "";
  left = "";
  top = "";
  width = "";
  height = "";
}

class FakeElement {
  readonly style = new FakeStyle() as CSSStyleDeclaration;
  readonly children: FakeElement[] = [];
  className = "";
  parent: FakeElement | null = null;

  appendChild(child: FakeElement): FakeElement {
    child.parent = this;
    this.children.push(child);
    return child;
  }

  remove(): void {
    if (this.parent == null) return;
    const index = this.parent.children.indexOf(this);
    if (index >= 0) this.parent.children.splice(index, 1);
    this.parent = null;
  }
}

class FakeDocument {
  readonly body = new FakeElement();

  createElement(): FakeElement {
    return new FakeElement();
  }
}

class FakeCanvas {
  readonly ownerDocument = new FakeDocument() as unknown as Document;
  readonly listeners: FakeListener[] = [];

  addEventListener(type: string, fn: (event: MouseEvent) => void): void {
    this.listeners.push({ type, fn });
  }

  removeEventListener(type: string, fn: (event: MouseEvent) => void): void {
    const index = this.listeners.findIndex((listener) => listener.type === type && listener.fn === fn);
    if (index >= 0) this.listeners.splice(index, 1);
  }

  getBoundingClientRect() {
    return { left: 0, top: 0, width: 800, height: 600 };
  }

  body(): FakeElement {
    return (this.ownerDocument.body as unknown) as FakeElement;
  }
}

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

let capturedScene: THREE.Scene | null = null;

function SceneProbe() {
  capturedScene = useThree((state) => state.scene);
  return null;
}

describe("selection HUD R3F bindings", () => {
  test("MarqueeOverlay mounts and unmounts the vanilla overlay listeners", async () => {
    const canvas = new FakeCanvas();
    const renderer = await create(
      createElement(MarqueeOverlay, { canvas }),
    );

    expect(canvas.listeners.map((listener) => listener.type).sort()).toEqual([
      "mousedown",
      "mouseleave",
      "mousemove",
      "mouseup",
    ]);

    await act(async () => {
      await renderer.unmount();
    });

    expect(canvas.listeners).toHaveLength(0);
    expect(canvas.body().children).toHaveLength(0);
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
