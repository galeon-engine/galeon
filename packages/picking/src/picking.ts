// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import {
  GALEON_ENTITY_KEY,
  GALEON_INSTANCE_ENTITIES_KEY,
  type GaleonInstancedMesh,
  type InstancedEntityResolver,
} from "@galeon/three";
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

/** Point hit returned by a picking backend. */
export interface PickingPointHit {
  readonly entity: PickingEntityRef;
  readonly point: { readonly x: number; readonly y: number; readonly z: number };
}

export type PickingFilter = (object: THREE.Object3D, entity: PickingEntityRef) => boolean;

export interface PickAtRequest {
  readonly scene: THREE.Scene;
  readonly camera: THREE.Camera;
  readonly ndc: THREE.Vector2;
}

export interface PickRectRequest {
  readonly scene: THREE.Scene;
  readonly camera: THREE.Camera;
  readonly ndcStart: THREE.Vector2;
  readonly ndcEnd: THREE.Vector2;
  readonly filter?: PickingFilter;
}

export interface PickingBackend {
  pickAt(request: PickAtRequest): PickingPointHit | null;
  pickRect(request: PickRectRequest): readonly PickingEntityRef[];
}

export type PickingBackendSelection = "galeon" | "raycaster" | PickingBackend;
export type PickingBackendOption = PickingBackendSelection | (() => PickingBackendSelection);

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
   * Picking implementation used for click and drag-rectangle queries.
   *
   * Pass `"raycaster"` or omit the option for the built-in Three.js raycaster
   * backend. A provider function is evaluated at pick time, which lets callers
   * switch between compatible backends without reattaching DOM listeners.
   */
  readonly pickingBackend?: PickingBackendOption;
  /**
   * Optional filter for marquee candidates. Receives every managed object
   * with a `GALEON_ENTITY_KEY` stamp; return `false` to exclude it (e.g.,
   * disable picking on UI gizmos).
   */
  readonly filter?: PickingFilter;
}

export function createRaycasterPickingBackend(): PickingBackend {
  const raycaster = new THREE.Raycaster();
  return {
    pickAt({ scene, camera, ndc }) {
      return pickPoint(scene, camera, ndc, raycaster);
    },
    pickRect({ scene, camera, ndcStart, ndcEnd, filter }) {
      return pickRect(scene, camera, ndcStart, ndcEnd, filter);
    },
  };
}

export function createGaleonPickingBackend(): PickingBackend {
  const raycaster = new THREE.Raycaster();
  return {
    pickAt({ scene, camera, ndc }) {
      const standalone = pickPoint(scene, camera, ndc, raycaster, isBvhInstancedMesh);
      const instanced = pickInstancedBvhPoint(scene, camera, raycaster);
      if (standalone == null) return instanced?.hit ?? null;
      if (instanced == null) return standalone;
      const standaloneDistance = pointDistance(raycaster.ray.origin, standalone.point);
      return instanced.distance < standaloneDistance ? instanced.hit : standalone;
    },
    pickRect({ scene, camera, ndcStart, ndcEnd, filter }) {
      const standalone = pickRect(scene, camera, ndcStart, ndcEnd, filter, isBvhInstancedMesh);
      const instanced = pickInstancedBvhRect(scene, camera, ndcStart, ndcEnd, filter);
      return [...standalone, ...instanced];
    },
  };
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
  const galeonBackend = createGaleonPickingBackend();
  const raycasterBackend = createRaycasterPickingBackend();
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
      const entities = currentPickingBackend(
        options.pickingBackend,
        galeonBackend,
        raycasterBackend,
      ).pickRect({
        scene,
        camera,
        ndcStart,
        ndcEnd,
        filter,
      });
      onPick?.({ kind: "pick-rect", entities, modifiers });
    } else {
      const ndcClick = new THREE.Vector2();
      toNdc(canvas, event.clientX, event.clientY, ndcClick);
      const result = currentPickingBackend(
        options.pickingBackend,
        galeonBackend,
        raycasterBackend,
      ).pickAt({
        scene,
        camera,
        ndc: ndcClick,
      });
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

function currentPickingBackend(
  option: PickingBackendOption | undefined,
  galeonBackend: PickingBackend,
  raycasterBackend: PickingBackend,
): PickingBackend {
  const selection = typeof option === "function" ? option() : option;
  if (selection == null || selection === "galeon") return galeonBackend;
  if (selection === "raycaster") return raycasterBackend;
  return selection;
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
  skipObject?: (object: THREE.Object3D) => boolean,
): PickingPointHit | null {
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
    if (skipObject?.(hit.object)) continue;
    const instanceEntity = visibleInstanceEntity(hit);
    if (instanceEntity != null) {
      const p = hit.point;
      return { entity: instanceEntity, point: { x: p.x, y: p.y, z: p.z } };
    }
    const entity = visibleAncestorEntity(hit.object);
    if (entity != null) {
      const p = hit.point;
      return { entity, point: { x: p.x, y: p.y, z: p.z } };
    }
  }
  return null;
}

function visibleInstanceEntity(hit: THREE.Intersection): PickingEntityRef | null {
  if (!visibleObjectChain(hit.object)) return null;
  return readInstanceEntity(hit);
}

function readInstanceEntity(hit: THREE.Intersection): PickingEntityRef | null {
  const instanceId = hit.instanceId;
  if (instanceId == null) return null;
  const data = hit.object.userData as Record<PropertyKey, unknown> | undefined;
  const resolver = data?.[GALEON_INSTANCE_ENTITIES_KEY] as
    | InstancedEntityResolver
    | undefined;
  const entity = resolver?.entityAt(instanceId);
  if (entity == null) return null;
  return entity;
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
  filter: PickingFilter | undefined,
  skipObject?: (object: THREE.Object3D) => boolean,
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
    if (skipObject?.(object)) return;
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

const _instancedLocalRaycaster = new THREE.Raycaster();
const _instancedInverseWorld = new THREE.Matrix4();
const _instancedMatrix = new THREE.Matrix4();
const _instancedWorldMatrix = new THREE.Matrix4();
const _instancedProbe = new THREE.Mesh();
const _instancedIntersections: THREE.Intersection[] = [];
const _cropMatrix = new THREE.Matrix4();
const _projectionMatrix = new THREE.Matrix4();

interface InstancedPointCandidate {
  readonly hit: PickingPointHit;
  readonly distance: number;
}

function pickInstancedBvhPoint(
  scene: THREE.Scene,
  camera: THREE.Camera,
  raycaster: THREE.Raycaster,
): InstancedPointCandidate | null {
  let best: InstancedPointCandidate | null = null;
  visitBvhInstancedMeshes(scene, camera, (mesh) => {
    const resolver = readInstanceResolver(mesh);
    if (resolver == null || mesh.bvh == null) return;

    _instancedInverseWorld.copy(mesh.matrixWorld).invert();
    _instancedLocalRaycaster.ray.copy(raycaster.ray).applyMatrix4(_instancedInverseWorld);
    _instancedLocalRaycaster.near = raycaster.near;
    _instancedLocalRaycaster.far = raycaster.far;
    _instancedLocalRaycaster.layers.mask = raycaster.layers.mask;

    mesh.bvh.raycast(_instancedLocalRaycaster, (instanceId) => {
      if (!mesh.getActiveAndVisibilityAt(instanceId)) return;
      const entity = resolver.entityAt(instanceId);
      if (entity == null) return;
      mesh.getMatrixAt(instanceId, _instancedMatrix);
      _instancedWorldMatrix.multiplyMatrices(mesh.matrixWorld, _instancedMatrix);
      _instancedProbe.geometry = mesh.geometry;
      _instancedProbe.material = Array.isArray(mesh.material) ? mesh.material[0]! : mesh.material;
      _instancedProbe.matrixWorld.copy(_instancedWorldMatrix);
      _instancedIntersections.length = 0;
      _instancedProbe.raycast(raycaster, _instancedIntersections);
      for (const preciseHit of _instancedIntersections) {
        const p = preciseHit.point;
        const distance = p.distanceTo(raycaster.ray.origin);
        if (best != null && distance >= best.distance) continue;
        best = {
          distance,
          hit: {
            entity,
            point: { x: p.x, y: p.y, z: p.z },
          },
        };
      }
    });
  });
  return best;
}

function pickInstancedBvhRect(
  scene: THREE.Scene,
  camera: THREE.Camera,
  ndcStart: THREE.Vector2,
  ndcEnd: THREE.Vector2,
  filter: PickingFilter | undefined,
): PickingEntityRef[] {
  camera.updateMatrixWorld();
  const out: PickingEntityRef[] = [];
  makeCropMatrix(ndcStart, ndcEnd, _cropMatrix);
  visitBvhInstancedMeshes(scene, camera, (mesh) => {
    const resolver = readInstanceResolver(mesh);
    if (resolver == null || mesh.bvh == null) return;
    _projectionMatrix
      .multiplyMatrices(_cropMatrix, camera.projectionMatrix)
      .multiply(camera.matrixWorldInverse)
      .multiply(mesh.matrixWorld);
    mesh.bvh.frustumCulling(_projectionMatrix, (node) => {
      const instanceId = node.object;
      if (instanceId == null) return;
      if (!mesh.getActiveAndVisibilityAt(instanceId)) return;
      const entity = resolver.entityAt(instanceId);
      if (entity == null) return;
      if (filter != null && !filter(mesh, entity)) return;
      out.push(entity);
    });
  });
  return out;
}

function visitBvhInstancedMeshes(
  scene: THREE.Scene,
  camera: THREE.Camera,
  visit: (mesh: GaleonInstancedMesh) => void,
): void {
  scene.traverse((object) => {
    if (!isBvhInstancedMesh(object)) return;
    if (!visibleObjectChain(object)) return;
    if (!object.layers.test(camera.layers)) return;
    visit(object);
  });
}

function isBvhInstancedMesh(object: THREE.Object3D): object is GaleonInstancedMesh {
  return (
    (object as { readonly isInstancedMesh2?: unknown }).isInstancedMesh2 === true &&
    (object as { readonly bvh?: unknown }).bvh != null
  );
}

function readInstanceResolver(mesh: GaleonInstancedMesh): InstancedEntityResolver | undefined {
  const data = mesh.userData as Record<PropertyKey, unknown> | undefined;
  return data?.[GALEON_INSTANCE_ENTITIES_KEY] as InstancedEntityResolver | undefined;
}

function makeCropMatrix(
  ndcStart: THREE.Vector2,
  ndcEnd: THREE.Vector2,
  out: THREE.Matrix4,
): THREE.Matrix4 {
  const minX = Math.min(ndcStart.x, ndcEnd.x);
  const maxX = Math.max(ndcStart.x, ndcEnd.x);
  const minY = Math.min(ndcStart.y, ndcEnd.y);
  const maxY = Math.max(ndcStart.y, ndcEnd.y);
  const width = Math.max(maxX - minX, Number.EPSILON);
  const height = Math.max(maxY - minY, Number.EPSILON);
  const sx = 2 / width;
  const sy = 2 / height;
  const tx = -(maxX + minX) / width;
  const ty = -(maxY + minY) / height;
  return out.set(
    sx, 0, 0, tx,
    0, sy, 0, ty,
    0, 0, 1, 0,
    0, 0, 0, 1,
  );
}

function pointDistance(
  origin: THREE.Vector3,
  point: { readonly x: number; readonly y: number; readonly z: number },
): number {
  const dx = point.x - origin.x;
  const dy = point.y - origin.y;
  const dz = point.z - origin.z;
  return Math.sqrt(dx * dx + dy * dy + dz * dz);
}

/**
 * Compute a world-space AABB for a stamped object.
 *
 * Unions every contributing geometry under `object` into a world-space AABB:
 * the stamped object's own geometry (if it is a `Mesh` / `LineSegments`) plus
 * every visible descendant geometry that is not under another stamped entity.
 * Click picking is recursive (`intersectObjects(scene.children, true)`) and
 * walks hits up to the nearest stamped ancestor, so the marquee bounds must
 * include the same descendant geometry — otherwise a stamped Mesh or Group
 * with offset unstamped child meshes is selectable by click but missed by
 * drag-rectangle.
 *
 * Falls back to a zero-size box at the object's world origin when no
 * descendant geometry exists (lights, empty groups), so they still pick when
 * the rect covers their position — but only if the stamped object's own
 * layer mask overlaps the camera.
 *
 * Pruning rules during the walk:
 * - Hidden subtrees are skipped wholesale (visibility is inherited).
 * - Nested stamped entities terminate the walk: click resolves to the nearest
 *   stamped ancestor, so a stamped child's geometry belongs to that child's
 *   bounds, not its stamped parent's.
 * - Per-mesh layer test prunes contributions whose own layer mask does not
 *   overlap the camera (layers are NOT inherited in Three.js), but traversal
 *   continues into children regardless because a layer-mismatched parent can
 *   still own visible-on-camera descendants.
 */
function worldAabb(
  object: THREE.Object3D,
  out: THREE.Box3,
  camera: THREE.Camera,
): THREE.Box3 | null {
  out.makeEmpty();
  const tmp = new THREE.Box3();
  let hasGeometry = false;

  function contribute(node: THREE.Object3D): void {
    if (!(node instanceof THREE.Mesh) && !(node instanceof THREE.LineSegments)) return;
    if (!node.layers.test(camera.layers)) return;
    const geom = node.geometry;
    if (geom == null) return;
    if (geom.boundingBox === null) {
      geom.computeBoundingBox();
    }
    if (geom.boundingBox === null) return;
    tmp.copy(geom.boundingBox).applyMatrix4(node.matrixWorld);
    out.union(tmp);
    hasGeometry = true;
  }

  function visit(node: THREE.Object3D): void {
    if (node !== object) {
      if (!node.visible) return;
      if (readEntity(node) != null) return;
    }
    contribute(node);
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

function visibleObjectChain(object: THREE.Object3D): boolean {
  let current: THREE.Object3D | null = object;
  while (current != null) {
    if (!current.visible) return false;
    current = current.parent;
  }
  return true;
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
