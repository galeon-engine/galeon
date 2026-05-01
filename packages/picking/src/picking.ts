// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import { GALEON_ENTITY_KEY } from "@galeon/three";
import { frustumFromRect } from "./selection-frustum.js";

/**
 * Stable entity reference stamped on managed Three.js objects by `RendererCache`.
 *
 * Mirrors the `{ entityId, generation }` payload that `@galeon/three` writes to
 * `userData[GALEON_ENTITY_KEY]`. Picking events surface this so consumers can
 * drop the reference into a Rust-side `Selection` without re-resolving anything.
 */
export interface PickingEntityRef {
  readonly entityId: number;
  readonly generation: number;
}

/** Modifier-key state captured at the originating mouse event. */
export interface PickModifiers {
  readonly shift: boolean;
  readonly ctrl: boolean;
  readonly alt: boolean;
  readonly meta: boolean;
}

/** Single-click pick result. */
export interface PickEvent {
  readonly kind: "pick";
  /** Entity hit by the ray, or `null` if no managed object intersected. */
  readonly entity: PickingEntityRef | null;
  /** World-space hit point, or `null` if no intersection. */
  readonly point: { readonly x: number; readonly y: number; readonly z: number } | null;
  readonly modifiers: PickModifiers;
}

/** Drag-rectangle marquee pick result. */
export interface PickRectEvent {
  readonly kind: "pick-rect";
  /** All managed entities whose AABB intersects the rect frustum. */
  readonly entities: readonly PickingEntityRef[];
  readonly modifiers: PickModifiers;
}

export type PickingEvent = PickEvent | PickRectEvent;

/**
 * Subset of `HTMLCanvasElement` that picking actually depends on.
 *
 * Lets adapters and tests pass a minimal stub. Real callers pass the
 * `<canvas>` element used by the WebGL renderer.
 */
export interface PickingTarget {
  addEventListener(
    type: string,
    listener: (event: MouseEvent) => void,
    options?: AddEventListenerOptions | boolean,
  ): void;
  removeEventListener(
    type: string,
    listener: (event: MouseEvent) => void,
    options?: EventListenerOptions | boolean,
  ): void;
  getBoundingClientRect(): { left: number; top: number; width: number; height: number };
}

export interface PickingOptions {
  /** Receives every emitted `pick` and `pick-rect` event. */
  readonly onPick?: (event: PickingEvent) => void;
  /** Pixels of mouse movement before drag becomes a marquee. Default: 4. */
  readonly dragThreshold?: number;
  /**
   * Optional filter for marquee candidates. Receives every managed object
   * with a `GALEON_ENTITY_KEY` stamp; return `false` to exclude it (e.g.,
   * disable picking on UI gizmos).
   */
  readonly filter?: (object: THREE.Object3D, entity: PickingEntityRef) => boolean;
}

/**
 * Wire mouse picking + drag-rectangle marquee to a `<canvas>` element.
 *
 * The returned disposer detaches every listener; call it on unmount or when
 * swapping cameras / scenes. Owns no GPU resources and never modifies the scene.
 *
 * Behaviour:
 * - **Click (no drag)** → emits `{ kind: "pick", entity, point, modifiers }`.
 *   The first intersection whose ancestor chain carries a `GALEON_ENTITY_KEY`
 *   stamp wins; `entity` is `null` if no managed object is hit.
 * - **Drag past `dragThreshold`** → emits `{ kind: "pick-rect", entities, modifiers }`
 *   on mouse-up. AABBs of every managed object are tested against a six-plane
 *   sub-frustum derived from the rect.
 *
 * Modifier-key conventions (StarCraft / OpenRA consensus, see issue #214 discovery):
 * `shift` = additive, `ctrl` = subtractive, `alt` = intersect. The helper only
 * **reports** them — applying them against an existing selection is the consumer's job.
 */
export function attachPicking(
  canvas: PickingTarget,
  scene: THREE.Scene,
  camera: THREE.Camera,
  options: PickingOptions = {},
): () => void {
  const dragThreshold = options.dragThreshold ?? 4;
  const onPick = options.onPick;
  const filter = options.filter;
  const raycaster = new THREE.Raycaster();
  const ndcStart = new THREE.Vector2();
  const ndcEnd = new THREE.Vector2();

  let pressed = false;
  let dragging = false;
  let downClientX = 0;
  let downClientY = 0;

  function onMouseDown(event: MouseEvent): void {
    if (event.button !== 0) return;
    pressed = true;
    dragging = false;
    downClientX = event.clientX;
    downClientY = event.clientY;
    toNdc(canvas, event.clientX, event.clientY, ndcStart);
  }

  function onMouseMove(event: MouseEvent): void {
    if (!pressed) return;
    const dx = event.clientX - downClientX;
    const dy = event.clientY - downClientY;
    if (!dragging && dx * dx + dy * dy >= dragThreshold * dragThreshold) {
      dragging = true;
    }
  }

  function onMouseUp(event: MouseEvent): void {
    if (event.button !== 0) return;
    if (!pressed) return;
    pressed = false;
    const modifiers = readModifiers(event);
    if (dragging) {
      toNdc(canvas, event.clientX, event.clientY, ndcEnd);
      const entities = pickRect(scene, camera, ndcStart, ndcEnd, filter);
      onPick?.({ kind: "pick-rect", entities, modifiers });
    } else {
      const ndcClick = new THREE.Vector2();
      toNdc(canvas, event.clientX, event.clientY, ndcClick);
      const result = pickPoint(scene, camera, ndcClick, raycaster);
      onPick?.({
        kind: "pick",
        entity: result?.entity ?? null,
        point: result?.point ?? null,
        modifiers,
      });
    }
    dragging = false;
  }

  // mouseleave defuses an in-progress drag if the cursor exits the canvas.
  // Without this, releasing outside the canvas would leave `pressed` true and
  // the next mousedown would mis-classify as the start of a new drag.
  function onMouseLeave(): void {
    pressed = false;
    dragging = false;
  }

  canvas.addEventListener("mousedown", onMouseDown);
  canvas.addEventListener("mousemove", onMouseMove);
  canvas.addEventListener("mouseup", onMouseUp);
  canvas.addEventListener("mouseleave", onMouseLeave);

  return () => {
    canvas.removeEventListener("mousedown", onMouseDown);
    canvas.removeEventListener("mousemove", onMouseMove);
    canvas.removeEventListener("mouseup", onMouseUp);
    canvas.removeEventListener("mouseleave", onMouseLeave);
  };
}

/**
 * Convert client-space pixels to Normalised Device Coordinates relative to a
 * canvas. Uses `getBoundingClientRect()` (not `innerWidth`) so non-fullscreen
 * canvases work correctly. DPR is intentionally not applied — NDC is unitless.
 */
function toNdc(
  canvas: PickingTarget,
  clientX: number,
  clientY: number,
  out: THREE.Vector2,
): void {
  const rect = canvas.getBoundingClientRect();
  out.x = ((clientX - rect.left) / rect.width) * 2 - 1;
  out.y = -((clientY - rect.top) / rect.height) * 2 + 1;
}

function readModifiers(event: MouseEvent): PickModifiers {
  return {
    shift: event.shiftKey,
    ctrl: event.ctrlKey,
    alt: event.altKey,
    meta: event.metaKey,
  };
}

interface PickPointResult {
  entity: PickingEntityRef;
  point: { x: number; y: number; z: number };
}

/**
 * Cast a single ray through `ndc` and return the first intersection whose
 * ancestor chain carries a `GALEON_ENTITY_KEY` stamp.
 *
 * Walking ancestors is necessary because `RendererCache` stamps the top-level
 * managed object. Sub-meshes of a Group hit by the ray must resolve back to
 * their owning entity.
 */
function pickPoint(
  scene: THREE.Scene,
  camera: THREE.Camera,
  ndc: THREE.Vector2,
  raycaster: THREE.Raycaster,
): PickPointResult | null {
  raycaster.setFromCamera(ndc, camera);
  const intersections = raycaster.intersectObjects(scene.children, true);
  for (const hit of intersections) {
    const entity = ancestorEntity(hit.object);
    if (entity != null) {
      const p = hit.point;
      return { entity, point: { x: p.x, y: p.y, z: p.z } };
    }
  }
  return null;
}

/**
 * Test every managed object against a six-plane sub-frustum derived from the
 * rect, returning entity refs for those whose world-space AABB intersects.
 *
 * `updateMatrixWorld(true)` runs first because Three skips dirty children
 * silently — without it, frustum tests can read stale world matrices for
 * objects that moved this frame.
 */
function pickRect(
  scene: THREE.Scene,
  camera: THREE.Camera,
  ndcStart: THREE.Vector2,
  ndcEnd: THREE.Vector2,
  filter: PickingOptions["filter"],
): PickingEntityRef[] {
  scene.updateMatrixWorld(true);
  const frustum = frustumFromRect(ndcStart, ndcEnd, camera);
  const aabb = new THREE.Box3();
  const out: PickingEntityRef[] = [];
  scene.traverse((object) => {
    const entity = readEntity(object);
    if (entity == null) return;
    if (filter && !filter(object, entity)) return;
    if (worldAabb(object, aabb) === null) return;
    if (frustum.intersectsBox(aabb)) {
      out.push(entity);
    }
  });
  return out;
}

/**
 * Compute a world-space AABB for an object. Falls back to a tight AABB around
 * the object's world-space origin if it has no geometry (lights, groups), so
 * lights inside the rect still pick.
 */
function worldAabb(object: THREE.Object3D, out: THREE.Box3): THREE.Box3 | null {
  if (object instanceof THREE.Mesh || object instanceof THREE.LineSegments) {
    const geom = object.geometry;
    if (geom == null) return null;
    if (geom.boundingBox === null) {
      geom.computeBoundingBox();
    }
    if (geom.boundingBox === null) return null;
    out.copy(geom.boundingBox).applyMatrix4(object.matrixWorld);
    return out;
  }
  // Non-mesh managed objects (lights, groups): use a zero-size box at world origin.
  const pos = new THREE.Vector3();
  object.getWorldPosition(pos);
  out.set(pos, pos);
  return out;
}

function readEntity(object: THREE.Object3D): PickingEntityRef | null {
  const data = object.userData as Record<PropertyKey, unknown> | undefined;
  if (data == null) return null;
  const stamped = data[GALEON_ENTITY_KEY] as { entityId?: unknown; generation?: unknown } | undefined;
  if (stamped == null) return null;
  if (typeof stamped.entityId !== "number" || typeof stamped.generation !== "number") {
    return null;
  }
  return { entityId: stamped.entityId, generation: stamped.generation };
}

function ancestorEntity(object: THREE.Object3D): PickingEntityRef | null {
  let current: THREE.Object3D | null = object;
  while (current != null) {
    const entity = readEntity(current);
    if (entity != null) return entity;
    current = current.parent;
  }
  return null;
}
