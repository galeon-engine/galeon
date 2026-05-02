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
  scene.updateMatrixWorld(true);
  // `scene.updateMatrixWorld` does not touch a camera that lives outside the
  // scene graph, and `Camera.updateMatrixWorld` is what refreshes both
  // `matrixWorld` and `matrixWorldInverse` — both of which `setFromCamera`
  // reads. Without this, picks taken between a camera move and the next
  // render would use stale ray origins and select the wrong entity.
  camera.updateMatrixWorld();
  // Align the raycaster's layer mask with the active camera so picks ignore
  // objects the camera would not render. Cameras using non-default layers
  // (editor gizmo views, multi-pass setups) would otherwise hit the wrong
  // objects via the raycaster's default layer-0 mask.
  raycaster.layers.mask = camera.layers.mask;
  raycaster.setFromCamera(ndc, camera);
  const intersections = raycaster.intersectObjects(scene.children, true);
  for (const hit of intersections) {
    const entity = visibleAncestorEntity(hit.object);
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
  // `frustumFromRect` unprojects through `camera.matrixWorld` /
  // `projectionMatrixInverse`; the same stale-camera risk that affects
  // `pickPoint` applies here.
  camera.updateMatrixWorld();
  const frustum = frustumFromRect(ndcStart, ndcEnd, camera);
  const aabb = new THREE.Box3();
  const out: PickingEntityRef[] = [];
  // Manual walker — `scene.traverse` always recurses, but a hidden subtree must
  // be skipped wholesale to keep marquee semantics consistent with click picking.
  // Visibility is inherited in Three.js, so an invisible parent prunes the whole
  // subtree. Layers are NOT inherited — each object's own `layers.test(camera)`
  // governs whether it is rendered. A stamped parent on a non-camera layer can
  // still own visible-and-rendered children (which click would resolve back to
  // the parent), so the layer test does not gate candidacy here. Instead,
  // `worldAabb` returns `null` for a stamped Mesh whose own layer mismatches
  // and skips layer-mismatched contributions when walking descendants — that
  // is what filters out unrenderable entities.
  function visit(object: THREE.Object3D): void {
    if (!object.visible) return;
    const entity = readEntity(object);
    if (entity != null && (filter == null || filter(object, entity))) {
      const bounds = worldAabb(object, aabb, camera);
      if (bounds !== null && frustum.intersectsBox(bounds)) {
        out.push(entity);
      }
    }
    for (const child of object.children) visit(child);
  }
  for (const child of scene.children) visit(child);
  return out;
}

/**
 * Compute a world-space AABB for an object.
 *
 * For a `Mesh` / `LineSegments`, transforms its geometry box to world space.
 * For a `Group` (or any non-geometry object stamped as a managed entity),
 * unions every visible descendant geometry into a world-space AABB so that
 * grouped entities with offset child meshes marquee-select by their bounds,
 * not by the group origin alone.
 *
 * Falls back to a zero-size box at the object's world origin when no
 * descendant geometry exists (lights, empty groups), so they still pick when
 * the rect covers their position.
 *
 * Descendants whose `layers` mask does not overlap `camera.layers` are pruned
 * along with hidden subtrees so the bounds match what the renderer would
 * actually draw under this camera.
 */
function worldAabb(
  object: THREE.Object3D,
  out: THREE.Box3,
  camera: THREE.Camera,
): THREE.Box3 | null {
  if (object instanceof THREE.Mesh || object instanceof THREE.LineSegments) {
    // A stamped Mesh whose own layer mask does not overlap the camera is not
    // rendered, so it is also not selectable — return null so the candidate
    // walker drops it. Click picking already drops it via the raycaster's
    // synced layer mask; this keeps marquee aligned.
    if (!object.layers.test(camera.layers)) return null;
    const geom = object.geometry;
    if (geom == null) return null;
    if (geom.boundingBox === null) {
      geom.computeBoundingBox();
    }
    if (geom.boundingBox === null) return null;
    out.copy(geom.boundingBox).applyMatrix4(object.matrixWorld);
    return out;
  }

  out.makeEmpty();
  const tmp = new THREE.Box3();
  let hasGeometry = false;
  // Manual recursion — `Object3D.traverse` always descends, so a hidden
  // mid-tree node would still let its visible grandchildren expand the AABB.
  // Pruning the whole subtree at the first hidden ancestor matches what the
  // renderer would actually draw.
  //
  // Two more pruning rules:
  // - Stop at nested managed entities. Click picking resolves to the nearest
  //   stamped ancestor, so a stamped child's geometry belongs to *that*
  //   entity's bounds, not its stamped parent's. Including it here would
  //   inflate the parent's AABB and produce false-positive parent selections.
  // - Layers are not inherited in Three.js: only contribute geometry from
  //   meshes whose own layer mask overlaps the camera, but keep traversing
  //   into children regardless (children may be on different layers).
  function visit(node: THREE.Object3D): void {
    if (node !== object) {
      if (!node.visible) return;
      if (readEntity(node) != null) return;
    }
    if (node !== object && (node instanceof THREE.Mesh || node instanceof THREE.LineSegments)) {
      if (node.layers.test(camera.layers)) {
        const geom = node.geometry;
        if (geom != null) {
          if (geom.boundingBox === null) {
            geom.computeBoundingBox();
          }
          if (geom.boundingBox !== null) {
            tmp.copy(geom.boundingBox).applyMatrix4(node.matrixWorld);
            out.union(tmp);
            hasGeometry = true;
          }
        }
      }
    }
    for (const child of node.children) visit(child);
  }
  visit(object);
  if (hasGeometry) return out;

  // No geometry contributed (lights, empty groups, or all descendants pruned
  // by visibility/layer/nested-entity gates). Fall back to a zero-size box at
  // the world origin — but only if the stamped object's *own* layer mask
  // overlaps the camera. Without this gate, an empty Group or Light on a
  // non-camera layer would still be selectable by a marquee that covers its
  // origin, even though the renderer would skip it. Click picking can never
  // reach such an object (no geometry to ray-test), so marquee must drop it
  // too to keep the two paths consistent.
  if (!object.layers.test(camera.layers)) return null;
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

function visibleAncestorEntity(object: THREE.Object3D): PickingEntityRef | null {
  let current: THREE.Object3D | null = object;
  while (current != null) {
    if (!current.visible) return null;
    const entity = readEntity(current);
    if (entity != null) return entity;
    current = current.parent;
  }
  return null;
}
