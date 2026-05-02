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

function makeMeshAt(entityId: number, x: number, y: number, z: number): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
  mesh.position.set(x, y, z);
  (mesh.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = {
    entityId,
    generation: 0,
  };
  return mesh;
}

function makeOrthoCamera(): THREE.OrthographicCamera {
  // 4×4 frustum centred on the origin, looking down -Z.
  const camera = new THREE.OrthographicCamera(-2, 2, 2, -2, 0.1, 100);
  camera.position.set(0, 0, 5);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  return camera;
}

describe("attachPicking drag-rectangle marquee", () => {
  test("collects every entity whose AABB falls inside the rect", () => {
    const scene = new THREE.Scene();
    // 2x2 grid of cubes at (-1, -1, 0), (1, -1, 0), (-1, 1, 0), (1, 1, 0).
    const inside = makeMeshAt(1, 1, 1, 0);
    const outside = makeMeshAt(2, -10, -10, 0);
    scene.add(inside, outside);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 4,
    });

    // Drag a rect in the upper-right of the canvas — only `inside` should land.
    canvas.fire("mousedown", { clientX: 500, clientY: 100 });
    canvas.fire("mousemove", { clientX: 700, clientY: 200 });
    canvas.fire("mouseup", { clientX: 700, clientY: 200 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    expect(event.kind).toBe("pick-rect");
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    const ids = event.entities.map((e) => e.entityId).sort();
    expect(ids).toEqual([1]);
  });

  test("captures modifier keys for additive / subtractive / intersect semantics", () => {
    const scene = new THREE.Scene();
    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
    });

    canvas.fire("mousedown", { clientX: 100, clientY: 100, shiftKey: true });
    canvas.fire("mousemove", { clientX: 200, clientY: 200, shiftKey: true });
    canvas.fire("mouseup", { clientX: 200, clientY: 200, shiftKey: true });

    expect(events).toHaveLength(1);
    expect(events[0]!.modifiers).toEqual({
      shift: true,
      ctrl: false,
      alt: false,
      meta: false,
    });
  });

  test("treats movement under threshold as click, not marquee", () => {
    const scene = new THREE.Scene();
    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 8,
    });

    canvas.fire("mousedown", { clientX: 100, clientY: 100 });
    canvas.fire("mousemove", { clientX: 102, clientY: 101 });
    canvas.fire("mouseup", { clientX: 102, clientY: 101 });

    expect(events).toHaveLength(1);
    expect(events[0]!.kind).toBe("pick");
  });

  test("filter excludes objects from marquee candidates", () => {
    const scene = new THREE.Scene();
    const a = makeMeshAt(1, 0, 0, 0);
    const b = makeMeshAt(2, 0.5, 0.5, 0);
    scene.add(a, b);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
      filter: (_, entity) => entity.entityId !== 2,
    });

    canvas.fire("mousedown", { clientX: 0, clientY: 0 });
    canvas.fire("mousemove", { clientX: 800, clientY: 600 });
    canvas.fire("mouseup", { clientX: 800, clientY: 600 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    const ids = event.entities.map((e) => e.entityId).sort();
    expect(ids).toEqual([1]);
  });

  test("selects a stamped Group by its child-mesh bounds, not its origin", () => {
    const scene = new THREE.Scene();
    // Group origin sits at the canvas centre but is NDC (0,0); the child mesh
    // sits in the upper-right of the canvas. Marquee covering only the
    // upper-right must still select the grouped entity.
    const group = new THREE.Group();
    group.position.set(0, 0, 0);
    (group.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = {
      entityId: 99,
      generation: 1,
    };
    const child = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    child.position.set(1, 1, 0);
    group.add(child);
    scene.add(group);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 4,
    });

    // Upper-right rect — covers child mesh, not the group origin.
    canvas.fire("mousedown", { clientX: 500, clientY: 100 });
    canvas.fire("mousemove", { clientX: 700, clientY: 200 });
    canvas.fire("mouseup", { clientX: 700, clientY: 200 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    const ids = event.entities.map((e) => e.entityId).sort();
    expect(ids).toEqual([99]);
  });

  test("excludes invisible entities from marquee results", () => {
    const scene = new THREE.Scene();
    const visible = makeMeshAt(1, 1, 1, 0);
    const hidden = makeMeshAt(2, 0.5, 0.5, 0);
    hidden.visible = false;
    scene.add(visible, hidden);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
    });

    canvas.fire("mousedown", { clientX: 0, clientY: 0 });
    canvas.fire("mousemove", { clientX: 800, clientY: 600 });
    canvas.fire("mouseup", { clientX: 800, clientY: 600 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    const ids = event.entities.map((e) => e.entityId).sort();
    expect(ids).toEqual([1]);
  });

  test("prunes hidden descendant branches when computing a group's AABB", () => {
    const scene = new THREE.Scene();
    // Stamped visible group whose only descendant geometry lives under a
    // hidden mid-tree branch. Marquee covering that grandchild's position
    // must NOT select the group — those bounds would never render.
    const group = new THREE.Group();
    group.position.set(0, 0, 0);
    (group.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = {
      entityId: 50,
      generation: 1,
    };
    const hiddenBranch = new THREE.Group();
    hiddenBranch.visible = false;
    const stowaway = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    stowaway.position.set(1, 1, 0);
    hiddenBranch.add(stowaway);
    group.add(hiddenBranch);
    scene.add(group);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 4,
    });

    // Upper-right rect — over the stowaway's position, away from the group origin.
    canvas.fire("mousedown", { clientX: 500, clientY: 100 });
    canvas.fire("mousemove", { clientX: 700, clientY: 200 });
    canvas.fire("mouseup", { clientX: 700, clientY: 200 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    expect(event.entities).toEqual([]);
  });

  test("excludes entities whose ancestor is invisible", () => {
    const scene = new THREE.Scene();
    const hiddenParent = new THREE.Group();
    hiddenParent.visible = false;
    const childEntity = makeMeshAt(7, 0, 0, 0);
    hiddenParent.add(childEntity);
    scene.add(hiddenParent);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
    });

    canvas.fire("mousedown", { clientX: 0, clientY: 0 });
    canvas.fire("mousemove", { clientX: 800, clientY: 600 });
    canvas.fire("mouseup", { clientX: 800, clientY: 600 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    expect(event.entities).toEqual([]);
  });

  test("refreshes the camera matrix for marquee picks after a camera move", () => {
    const scene = new THREE.Scene();
    // Cube near the origin and another to the right; we'll pan the camera so
    // a "centre" marquee covers the right cube only.
    const a = makeMeshAt(1, 0, 0, 0);
    const b = makeMeshAt(2, 5, 0, 0);
    scene.add(a, b);

    const camera = makeOrthoCamera();
    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, camera, {
      onPick: (e) => events.push(e),
      dragThreshold: 4,
    });

    // Pan camera onto `b` without calling updateMatrixWorld manually.
    camera.position.set(5, 0, 5);

    // Tight marquee around the canvas centre — should land on `b`.
    canvas.fire("mousedown", { clientX: 380, clientY: 280 });
    canvas.fire("mousemove", { clientX: 420, clientY: 320 });
    canvas.fire("mouseup", { clientX: 420, clientY: 320 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    const ids = event.entities.map((e) => e.entityId).sort();
    expect(ids).toEqual([2]);
  });

  test("mouseleave defuses an in-progress drag without emitting", () => {
    const scene = new THREE.Scene();
    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
    });

    canvas.fire("mousedown", { clientX: 100, clientY: 100 });
    canvas.fire("mousemove", { clientX: 300, clientY: 300 });
    canvas.fire("mouseleave", {});
    // After leave, a fresh up has nothing to dispatch.
    canvas.fire("mouseup", { clientX: 300, clientY: 300 });

    expect(events).toHaveLength(0);
  });

  test("excludes entities whose layers do not match the camera", () => {
    const scene = new THREE.Scene();
    // `a` is on the camera's default layer; `b` is on a layer the camera
    // does not render. Both fall inside the marquee rect.
    const a = makeMeshAt(1, 0, 0, 0);
    const b = makeMeshAt(2, 0.5, 0.5, 0);
    b.layers.set(2);
    scene.add(a, b);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
    });

    canvas.fire("mousedown", { clientX: 380, clientY: 280 });
    canvas.fire("mousemove", { clientX: 420, clientY: 320 });
    canvas.fire("mouseup", { clientX: 420, clientY: 320 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    const ids = event.entities.map((e) => e.entityId).sort();
    expect(ids).toEqual([1]);
  });

  test("prunes layer-mismatched descendants when computing a group's AABB", () => {
    const scene = new THREE.Scene();
    // Group entity at the origin with one in-rect mesh on the camera layer
    // and one far-away mesh on a non-rendered layer. Without layer-aware
    // AABB pruning, the far mesh would inflate the group's bounds and let
    // it match a rect that does not actually cover anything drawn.
    const group = new THREE.Group();
    (group.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = {
      entityId: 7,
      generation: 0,
    };
    const drawn = new THREE.Mesh(new THREE.BoxGeometry(0.2, 0.2, 0.2), new THREE.MeshBasicMaterial());
    drawn.position.set(0, 0, 0);
    const offscreen = new THREE.Mesh(
      new THREE.BoxGeometry(0.2, 0.2, 0.2),
      new THREE.MeshBasicMaterial(),
    );
    offscreen.position.set(10, 10, 0);
    offscreen.layers.set(2);
    group.add(drawn, offscreen);
    scene.add(group);

    const canvas = new FakeCanvas();
    const events: PickingEvent[] = [];
    attachPicking(canvas, scene, makeOrthoCamera(), {
      onPick: (e) => events.push(e),
      dragThreshold: 2,
    });

    // Tight marquee far away from the group's origin but where `offscreen`
    // sits in world space. With a layer-blind AABB this would still match.
    canvas.fire("mousedown", { clientX: 760, clientY: 0 });
    canvas.fire("mousemove", { clientX: 800, clientY: 40 });
    canvas.fire("mouseup", { clientX: 800, clientY: 40 });

    expect(events).toHaveLength(1);
    const event = events[0]!;
    if (event.kind !== "pick-rect") throw new Error("unreachable");
    expect(event.entities).toHaveLength(0);
  });
});
