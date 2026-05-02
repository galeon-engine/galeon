// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { GALEON_ENTITY_KEY } from "../../three/src/renderer-cache.js";
import { attachPicking, type PickingEvent, type PickingTarget } from "../src/picking.js";

interface FakeListener {
  type: string;
  fn: (event: MouseEvent) => void;
}

class FakeCanvas implements PickingTarget {
  readonly listeners: FakeListener[] = [];
  rect = { left: 0, top: 0, width: 800, height: 600 };

  addEventListener(type: string, fn: (event: MouseEvent) => void): void {
    this.listeners.push({ type, fn });
  }
  removeEventListener(type: string, fn: (event: MouseEvent) => void): void {
    const index = this.listeners.findIndex((l) => l.type === type && l.fn === fn);
    if (index >= 0) this.listeners.splice(index, 1);
  }
  getBoundingClientRect() {
    return this.rect;
  }
  fire(type: string, event: Partial<MouseEvent>): void {
    const full = { ...defaultMouseEvent(), ...event } as MouseEvent;
    for (const listener of this.listeners.filter((l) => l.type === type)) {
      listener.fn(full);
    }
  }
}

function defaultMouseEvent(): MouseEvent {
  return {
    button: 0,
    clientX: 0,
    clientY: 0,
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
  } as MouseEvent;
}

function stampEntity(object: THREE.Object3D, entityId: number, generation: number): void {
  (object.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = { entityId, generation };
}

function makeOrthoCamera(): THREE.OrthographicCamera {
  // Orthographic so a centred ray hits a unit cube at the origin reliably.
  const camera = new THREE.OrthographicCamera(-2, 2, 2, -2, 0.1, 100);
  camera.position.set(0, 0, 5);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  return camera;
}

describe("attachPicking single-click", () => {
  test("emits a pick event carrying the hit entity ref and world point", () => {
    const scene = new THREE.Scene();
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    stampEntity(mesh, 7, 1);
    scene.add(mesh);
    scene.updateMatrixWorld(true);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    const dispose = attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (event) => events.push(event),
    });

    canvas.fire("mousedown", { clientX: 400, clientY: 300 });
    canvas.fire("mouseup", { clientX: 400, clientY: 300 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    expect(event.kind).toBe("pick");
    if (event.kind !== "pick") throw new Error("unreachable");
    expect(event.entity).toEqual({ entityId: 7, generation: 1 });
    expect(event.point).not.toBeNull();
    // Camera at z=5 looking at origin, unit cube at origin → ray hits front face at z≈0.5.
    expect(event.point!.z).toBeGreaterThan(0.4);
    expect(event.point!.z).toBeLessThan(0.6);

    dispose();
    expect(canvas.listeners).toHaveLength(0);
  });

  test("returns null entity when the ray misses every managed object", () => {
    const scene = new THREE.Scene();
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    mesh.position.set(10, 0, 0); // out of view
    stampEntity(mesh, 1, 1);
    scene.add(mesh);
    scene.updateMatrixWorld(true);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    canvas.fire("mousedown", { clientX: 400, clientY: 300 });
    canvas.fire("mouseup", { clientX: 400, clientY: 300 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick") throw new Error("unreachable");
    expect(event.entity).toBeNull();
    expect(event.point).toBeNull();
  });

  test("walks ancestors so child sub-meshes resolve to their managed parent", () => {
    const scene = new THREE.Scene();
    const group = new THREE.Group();
    stampEntity(group, 42, 3);
    const child = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    group.add(child);
    scene.add(group);
    scene.updateMatrixWorld(true);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    canvas.fire("mousedown", { clientX: 400, clientY: 300 });
    canvas.fire("mouseup", { clientX: 400, clientY: 300 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick") throw new Error("unreachable");
    expect(event.entity).toEqual({ entityId: 42, generation: 3 });
  });

  test("refreshes scene matrices before raycasting dynamic objects", () => {
    const scene = new THREE.Scene();
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    mesh.position.set(10, 0, 0);
    stampEntity(mesh, 5, 1);
    scene.add(mesh);
    scene.updateMatrixWorld(true);

    mesh.position.set(0, 0, 0);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    canvas.fire("mousedown", { clientX: 400, clientY: 300 });
    canvas.fire("mouseup", { clientX: 400, clientY: 300 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick") throw new Error("unreachable");
    expect(event.entity).toEqual({ entityId: 5, generation: 1 });
  });

  test("skips invisible click hits and resolves the next visible entity", () => {
    const scene = new THREE.Scene();
    const hidden = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    hidden.visible = false;
    stampEntity(hidden, 1, 1);
    scene.add(hidden);

    const visible = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    visible.position.set(0, 0, -2);
    stampEntity(visible, 2, 1);
    scene.add(visible);
    scene.updateMatrixWorld(true);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    canvas.fire("mousedown", { clientX: 400, clientY: 300 });
    canvas.fire("mouseup", { clientX: 400, clientY: 300 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick") throw new Error("unreachable");
    expect(event.entity).toEqual({ entityId: 2, generation: 1 });
  });

  test("captures modifier-key state on the originating mouse-up", () => {
    const scene = new THREE.Scene();
    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    canvas.fire("mousedown", { clientX: 10, clientY: 10 });
    canvas.fire("mouseup", { clientX: 10, clientY: 10, shiftKey: true, altKey: true });

    expect(events).toHaveLength(1);
    expect(events[0]!.modifiers).toEqual({
      shift: true,
      ctrl: false,
      alt: true,
      meta: false,
    });
  });

  test("ignores non-primary mouse buttons", () => {
    const scene = new THREE.Scene();
    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    canvas.fire("mousedown", { clientX: 0, clientY: 0, button: 2 });
    canvas.fire("mouseup", { clientX: 0, clientY: 0, button: 2 });

    expect(events).toHaveLength(0);
  });

  test("non-fullscreen canvas math uses bounding rect, not window", () => {
    const scene = new THREE.Scene();
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    stampEntity(mesh, 9, 1);
    scene.add(mesh);
    scene.updateMatrixWorld(true);

    const canvas = new FakeCanvas();
    canvas.rect = { left: 100, top: 50, width: 400, height: 300 };
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), { onPick: (e) => events.push(e) });

    // Centre of the offset canvas: (300, 200) in client coords.
    canvas.fire("mousedown", { clientX: 300, clientY: 200 });
    canvas.fire("mouseup", { clientX: 300, clientY: 200 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick") throw new Error("unreachable");
    expect(event.entity).toEqual({ entityId: 9, generation: 1 });
  });
});
