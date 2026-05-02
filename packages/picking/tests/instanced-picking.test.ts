// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  INSTANCE_GROUP_NONE,
  RENDER_CONTRACT_VERSION,
  RendererCache,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "@galeon/three";
import { attachPicking, type PickingCandidate, type PickingEvent } from "../src/picking.js";

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
    packet.transforms[off + 6] = 1;
    packet.transforms[off + 7] = 1;
    packet.transforms[off + 8] = 1;
    packet.transforms[off + 9] = 1;
  }
}

class CanvasStub {
  private readonly listeners = new Map<string, Set<(event: MouseEvent) => void>>();

  addEventListener(type: string, listener: (event: MouseEvent) => void): void {
    let listeners = this.listeners.get(type);
    if (listeners === undefined) {
      listeners = new Set();
      this.listeners.set(type, listeners);
    }
    listeners.add(listener);
  }

  removeEventListener(type: string, listener: (event: MouseEvent) => void): void {
    this.listeners.get(type)?.delete(listener);
  }

  getBoundingClientRect(): { left: number; top: number; width: number; height: number } {
    return { left: 0, top: 0, width: 100, height: 100 };
  }

  dispatch(type: string, event: MouseEvent): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

function mouse(type: string, clientX: number, clientY: number): MouseEvent {
  return {
    type,
    button: 0,
    clientX,
    clientY,
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
  } as MouseEvent;
}

describe("@galeon/picking instanced mesh identity (#224)", () => {
  test("click pick resolves InstancedMesh instanceId to entity generation", () => {
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(70, 1, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();

    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 2 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 10;
    packet.entity_generations[0] = 3;
    packet.mesh_handles[0] = 7;
    packet.transforms[0] = 0;
    packet.transforms[1] = 0;
    packet.transforms[2] = 0;
    packet.entity_ids[1] = 11;
    packet.entity_generations[1] = 4;
    packet.mesh_handles[1] = 7;
    packet.transforms[TRANSFORM_STRIDE] = 3;
    packet.transforms[TRANSFORM_STRIDE + 1] = 0;
    packet.transforms[TRANSFORM_STRIDE + 2] = 0;
    packet.instance_groups = new Uint32Array([7, 7]);

    cache.applyFrame(packet);

    const canvas = new CanvasStub();
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, camera, {
      onPick: (event) => events.push(event),
    });

    canvas.dispatch("mousedown", mouse("mousedown", 50, 50));
    canvas.dispatch("mouseup", mouse("mouseup", 50, 50));
    dispose();

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick");
    if (events[0]!.kind !== "pick") throw new Error("expected pick event");
    expect(events[0]!.entity).toEqual({ entityId: 10, generation: 3 });
    expect(events[0]!.point).not.toBeNull();
  });

  test("click pick reports the closer overlapping instance without GPU readback", () => {
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(70, 1, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();

    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 2 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 10;
    packet.entity_generations[0] = 3;
    packet.mesh_handles[0] = 7;
    packet.transforms[2] = 0;
    packet.entity_ids[1] = 11;
    packet.entity_generations[1] = 4;
    packet.mesh_handles[1] = 7;
    packet.transforms[TRANSFORM_STRIDE + 2] = 1;
    packet.instance_groups = new Uint32Array([7, 7]);

    cache.applyFrame(packet);

    const canvas = new CanvasStub();
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, camera, {
      onPick: (event) => events.push(event),
    });

    canvas.dispatch("mousedown", mouse("mousedown", 50, 50));
    canvas.dispatch("mouseup", mouse("mouseup", 50, 50));
    dispose();

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick");
    if (events[0]!.kind !== "pick") throw new Error("expected pick event");
    expect(events[0]!.entity).toEqual({ entityId: 11, generation: 4 });
  });

  test("drag-rectangle marquee resolves InstancedMesh2 BVH hits per instance", () => {
    const scene = new THREE.Scene();
    const camera = new THREE.OrthographicCamera(-4, 4, 4, -4, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();

    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 3 });
    fillIdentityTransforms(packet);
    for (let i = 0; i < 3; i++) {
      packet.entity_ids[i] = 20 + i;
      packet.entity_generations[i] = 1;
      packet.mesh_handles[i] = 7;
    }
    packet.transforms[0] = -2;
    packet.transforms[TRANSFORM_STRIDE] = 2;
    packet.transforms[TRANSFORM_STRIDE * 2] = 6;
    packet.instance_groups = new Uint32Array([7, 7, 7]);
    cache.applyFrame(packet);

    const canvas = new CanvasStub();
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, camera, {
      dragThreshold: 2,
      onPick: (event) => events.push(event),
    });

    canvas.dispatch("mousedown", mouse("mousedown", 0, 0));
    canvas.dispatch("mousemove", mouse("mousemove", 75, 100));
    canvas.dispatch("mouseup", mouse("mouseup", 75, 100));
    dispose();

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick-rect");
    if (events[0]!.kind !== "pick-rect") throw new Error("expected pick-rect event");
    expect(events[0]!.entities.map((entity) => entity.entityId).sort()).toEqual([20, 21]);
  });

  test("drag-rectangle filter receives instanced candidates with instance identity", () => {
    const scene = new THREE.Scene();
    const camera = new THREE.OrthographicCamera(-4, 4, 4, -4, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();

    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 3 });
    fillIdentityTransforms(packet);
    for (let i = 0; i < 3; i++) {
      packet.entity_ids[i] = 20 + i;
      packet.entity_generations[i] = 1;
      packet.mesh_handles[i] = 7;
    }
    packet.transforms[0] = -2;
    packet.transforms[TRANSFORM_STRIDE] = 2;
    packet.transforms[TRANSFORM_STRIDE * 2] = 6;
    packet.instance_groups = new Uint32Array([7, 7, 7]);
    cache.applyFrame(packet);

    const canvas = new CanvasStub();
    const candidates: PickingCandidate[] = [];
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, camera, {
      dragThreshold: 2,
      filter: (candidate) => {
        candidates.push(candidate);
        return candidate.entity.entityId !== 21;
      },
      onPick: (event) => events.push(event),
    });

    canvas.dispatch("mousedown", mouse("mousedown", 0, 0));
    canvas.dispatch("mousemove", mouse("mousemove", 75, 100));
    canvas.dispatch("mouseup", mouse("mouseup", 75, 100));
    dispose();

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick-rect");
    if (events[0]!.kind !== "pick-rect") throw new Error("expected pick-rect event");
    expect(events[0]!.entities.map((entity) => entity.entityId).sort()).toEqual([20]);
    expect(candidates.map((candidate) => candidate.entity.entityId).sort()).toEqual([20, 21]);
    expect(candidates.every((candidate) => candidate.instanceId !== null)).toBe(true);
    expect(new Set(candidates.map((candidate) => candidate.object)).size).toBe(1);
  });

  test("click pick ignores hidden InstancedMesh hits", () => {
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(70, 1, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();

    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 10;
    packet.entity_generations[0] = 3;
    packet.mesh_handles[0] = 7;
    packet.instance_groups = new Uint32Array([7]);
    cache.applyFrame(packet);
    cache.instancing.meshFor(7)!.visible = false;

    const canvas = new CanvasStub();
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, camera, {
      onPick: (event) => events.push(event),
    });

    canvas.dispatch("mousedown", mouse("mousedown", 50, 50));
    canvas.dispatch("mouseup", mouse("mouseup", 50, 50));
    dispose();

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick");
    if (events[0]!.kind !== "pick") throw new Error("expected pick event");
    expect(events[0]!.entity).toBeNull();
    expect(events[0]!.point).toBeNull();
  });

  test("standalone picks still use object entity stamps", () => {
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(70, 1, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);

    const cache = new RendererCache(scene);
    cache.registerGeometry(7, new THREE.BoxGeometry(1, 1, 1));
    cache.registerMaterial(0, new THREE.MeshBasicMaterial());

    const packet = makePacket({ entity_count: 1 });
    fillIdentityTransforms(packet);
    packet.entity_ids[0] = 12;
    packet.entity_generations[0] = 5;
    packet.mesh_handles[0] = 7;
    packet.instance_groups = new Uint32Array([INSTANCE_GROUP_NONE]);
    cache.applyFrame(packet);

    const canvas = new CanvasStub();
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, camera, {
      onPick: (event) => events.push(event),
    });

    canvas.dispatch("mousedown", mouse("mousedown", 50, 50));
    canvas.dispatch("mouseup", mouse("mouseup", 50, 50));
    dispose();

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick");
    if (events[0]!.kind !== "pick") throw new Error("expected pick event");
    expect(events[0]!.entity).toEqual({ entityId: 12, generation: 5 });
  });
});
