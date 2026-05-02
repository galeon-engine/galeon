// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import { TRANSFORM_STRIDE } from "@galeon/render-core";

/** Initial slot capacity allocated when a new instance batch is created. */
const INITIAL_CAPACITY = 16;
/** Minimum capacity floor — keeps tiny batches from thrashing the grow path. */
const MIN_CAPACITY = 4;

/**
 * Per-`MeshHandle` `THREE.InstancedMesh` lifecycle:
 *
 * - **Lazy creation** — a batch is allocated the first time an entity with that
 *   group key arrives.
 * - **Grow-by-2× capacity** — when a batch fills up, a new `InstancedMesh` of
 *   double size replaces the old one in the scene. Old matrices are copied over.
 * - **Swap-with-last on remove** — the freed slot is filled by the entity at
 *   `count-1`, keeping the active range contiguous and `mesh.count` rendering
 *   exactly the live instances.
 *
 * The manager does *not* read render-packet contracts directly — callers
 * pre-resolve `geometry` / `material` from the registries and pass them in.
 * This keeps registry logic in `RendererCache` and avoids duplicating fallback
 * rules (placeholder, missing-handle warnings, override-survival semantics).
 */
interface Batch {
  readonly groupKey: number;
  mesh: THREE.InstancedMesh;
  capacity: number;
  count: number;
  /** `slot → entityId` (`-1` for empty slots, but only `[0, count)` is meaningful). */
  slotToEntity: Int32Array;
  /** Last registry-resolved geometry and material for this batch. */
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
}

const _scratchPos = new THREE.Vector3();
const _scratchQuat = new THREE.Quaternion();
const _scratchScale = new THREE.Vector3();
const _scratchMatrix = new THREE.Matrix4();
const _scratchHidden = new THREE.Vector3(0, 0, 0);
const _scratchColor = new THREE.Color();

export class InstancedMeshManager {
  private readonly scene: THREE.Scene;
  private readonly batches = new Map<number, Batch>();
  /** `entityId → (groupKey, slot)` — single source of truth for placement. */
  private readonly entityToPlacement = new Map<
    number,
    { groupKey: number; slot: number }
  >();

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  /**
   * Insert or update an entity inside its instance batch.
   *
   * Migrations (entity already placed in a different batch) are handled by
   * removing from the old batch first; the matrix is then written into a
   * fresh slot in the new batch.
   *
   * Returns the `(groupKey, slot)` placement so the caller can stamp identity
   * back on the entity if needed.
   */
  upsert(
    entityId: number,
    groupKey: number,
    geometry: THREE.BufferGeometry,
    material: THREE.Material,
    transforms: Float32Array,
    transformIndex: number,
    visible: boolean,
    tints?: Float32Array,
  ): { groupKey: number; slot: number } {
    const existing = this.entityToPlacement.get(entityId);
    if (existing !== undefined && existing.groupKey !== groupKey) {
      this.remove(entityId);
    }

    const batch = this.getOrCreateBatch(groupKey, geometry, material);

    let slot: number;
    const placement = this.entityToPlacement.get(entityId);
    if (placement !== undefined) {
      slot = placement.slot;
    } else {
      if (batch.count === batch.capacity) {
        this.growBatch(batch);
      }
      slot = batch.count;
      batch.slotToEntity[slot] = entityId;
      batch.count += 1;
      batch.mesh.count = batch.count;
      this.entityToPlacement.set(entityId, { groupKey, slot });
    }

    this.writeSlotMatrix(batch, slot, transforms, transformIndex, visible);
    if (tints !== undefined) {
      this.writeSlotTint(batch, slot, tints, transformIndex);
    }
    return { groupKey, slot };
  }

  /**
   * Remove an entity from its instance batch, if any. Returns whether
   * the entity was actually placed.
   *
   * Uses swap-with-last to keep `[0, count)` contiguous without re-uploading
   * unrelated slots.
   */
  remove(entityId: number): boolean {
    const placement = this.entityToPlacement.get(entityId);
    if (placement === undefined) return false;
    const batch = this.batches.get(placement.groupKey);
    if (batch === undefined) {
      this.entityToPlacement.delete(entityId);
      return false;
    }

    const { slot } = placement;
    const lastSlot = batch.count - 1;

    if (slot !== lastSlot) {
      // Copy last entity's matrix and color into the freed slot, then
      // update mappings.
      batch.mesh.getMatrixAt(lastSlot, _scratchMatrix);
      batch.mesh.setMatrixAt(slot, _scratchMatrix);
      batch.mesh.getColorAt(lastSlot, _scratchColor);
      batch.mesh.setColorAt(slot, _scratchColor);
      const movedEntity = batch.slotToEntity[lastSlot]!;
      batch.slotToEntity[slot] = movedEntity;
      this.entityToPlacement.set(movedEntity, {
        groupKey: batch.groupKey,
        slot,
      });
      batch.mesh.instanceMatrix.addUpdateRange(slot * 16, 16);
      if (batch.mesh.instanceColor) {
        batch.mesh.instanceColor.addUpdateRange(slot * 3, 3);
      }
    }

    batch.slotToEntity[lastSlot] = -1;
    batch.count = lastSlot;
    batch.mesh.count = batch.count;
    batch.mesh.instanceMatrix.needsUpdate = true;
    if (batch.mesh.instanceColor) batch.mesh.instanceColor.needsUpdate = true;
    this.entityToPlacement.delete(entityId);
    return true;
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  /** The `THREE.InstancedMesh` backing a group key, if one has been created. */
  meshFor(groupKey: number): THREE.InstancedMesh | undefined {
    return this.batches.get(groupKey)?.mesh;
  }

  /** Number of live instance slots in a group's batch (== `mesh.count`). */
  slotsFor(groupKey: number): number {
    return this.batches.get(groupKey)?.count ?? 0;
  }

  /** Allocated capacity of a group's batch — distinct from the live `slotsFor` count. */
  capacityFor(groupKey: number): number {
    return this.batches.get(groupKey)?.capacity ?? 0;
  }

  /** Number of distinct mesh-handle batches currently allocated. */
  get batchCount(): number {
    return this.batches.size;
  }

  /** Whether this entity is currently placed in any instance batch. */
  has(entityId: number): boolean {
    return this.entityToPlacement.has(entityId);
  }

  /**
   * Iterate over every entity currently held in any batch.
   *
   * Callers iterating to remove entities must collect first and mutate after
   * — this returns the live map's keys.
   */
  entityIds(): IterableIterator<number> {
    return this.entityToPlacement.keys();
  }

  // ---------------------------------------------------------------------------
  // Cleanup
  // ---------------------------------------------------------------------------

  /** Detach every batch from the scene and reset internal maps. */
  clear(): void {
    for (const batch of this.batches.values()) {
      batch.mesh.parent?.remove(batch.mesh);
      batch.mesh.dispose();
    }
    this.batches.clear();
    this.entityToPlacement.clear();
  }

  /** Alias for `clear()` — present so callers can mirror `RendererCache.dispose()`. */
  dispose(): void {
    this.clear();
  }

  // ---------------------------------------------------------------------------
  // Internal
  // ---------------------------------------------------------------------------

  private getOrCreateBatch(
    groupKey: number,
    geometry: THREE.BufferGeometry,
    material: THREE.Material,
  ): Batch {
    let batch = this.batches.get(groupKey);
    if (batch !== undefined) {
      if (batch.geometry !== geometry) {
        batch.mesh.geometry = geometry;
        batch.geometry = geometry;
      }
      if (batch.material !== material) {
        batch.mesh.material = material;
        batch.material = material;
      }
      return batch;
    }

    const capacity = INITIAL_CAPACITY;
    const mesh = new THREE.InstancedMesh(geometry, material, capacity);
    mesh.count = 0;
    mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    // Allocate `instanceColor` synchronously at creation, defaulting every
    // slot to white. First-time `instanceColor` writes trigger a shader
    // recompile in three.js (#21786), so doing it once at allocation —
    // rather than lazily on the first tinted entity — keeps the runtime
    // path stutter-free for tinted scenes. Untinted scenes pay only the
    // memory cost (12 bytes / capacity-slot), no shader cost.
    allocateInstanceColor(mesh, capacity);
    // Frustum culling is all-or-nothing per InstancedMesh in three.js (#27170).
    // Disable it so off-screen instances aren't suppressed at the mesh level.
    // A future per-instance culling backend (BVH) is out of scope for v1.
    mesh.frustumCulled = false;
    this.scene.add(mesh);

    batch = {
      groupKey,
      mesh,
      capacity,
      count: 0,
      slotToEntity: createSlotArray(capacity),
      geometry,
      material,
    };
    this.batches.set(groupKey, batch);
    return batch;
  }

  private growBatch(batch: Batch): void {
    const newCapacity = Math.max(batch.capacity * 2, MIN_CAPACITY);
    const newMesh = new THREE.InstancedMesh(
      batch.geometry,
      batch.material,
      newCapacity,
    );
    newMesh.count = batch.count;
    newMesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    allocateInstanceColor(newMesh, newCapacity);
    newMesh.frustumCulled = false;

    // Copy live matrices and colors from old to new.
    for (let s = 0; s < batch.count; s++) {
      batch.mesh.getMatrixAt(s, _scratchMatrix);
      newMesh.setMatrixAt(s, _scratchMatrix);
      batch.mesh.getColorAt(s, _scratchColor);
      newMesh.setColorAt(s, _scratchColor);
    }
    newMesh.instanceMatrix.needsUpdate = true;
    if (newMesh.instanceColor) newMesh.instanceColor.needsUpdate = true;

    // Swap into the scene and dispose the old buffer.
    batch.mesh.parent?.remove(batch.mesh);
    batch.mesh.dispose();
    this.scene.add(newMesh);

    const newSlots = createSlotArray(newCapacity);
    newSlots.set(batch.slotToEntity.subarray(0, batch.count));

    batch.mesh = newMesh;
    batch.capacity = newCapacity;
    batch.slotToEntity = newSlots;
  }

  private writeSlotTint(
    batch: Batch,
    slot: number,
    tints: Float32Array,
    transformIndex: number,
  ): void {
    const off = transformIndex * 3;
    _scratchColor.setRGB(tints[off]!, tints[off + 1]!, tints[off + 2]!);
    batch.mesh.setColorAt(slot, _scratchColor);
    if (batch.mesh.instanceColor) {
      batch.mesh.instanceColor.addUpdateRange(slot * 3, 3);
      batch.mesh.instanceColor.needsUpdate = true;
    }
  }

  private writeSlotMatrix(
    batch: Batch,
    slot: number,
    transforms: Float32Array,
    transformIndex: number,
    visible: boolean,
  ): void {
    const off = transformIndex * TRANSFORM_STRIDE;
    _scratchPos.set(
      transforms[off]!,
      transforms[off + 1]!,
      transforms[off + 2]!,
    );
    _scratchQuat.set(
      transforms[off + 3]!,
      transforms[off + 4]!,
      transforms[off + 5]!,
      transforms[off + 6]!,
    );
    if (visible) {
      _scratchScale.set(
        transforms[off + 7]!,
        transforms[off + 8]!,
        transforms[off + 9]!,
      );
    } else {
      // Hidden instances render with zero scale — keeps their slot stable for
      // a cheap re-show without disturbing surrounding entities. A future
      // visibility-aware swap (agargaro-style) can replace this if measured
      // perf calls for it.
      _scratchScale.copy(_scratchHidden);
    }
    _scratchMatrix.compose(_scratchPos, _scratchQuat, _scratchScale);
    batch.mesh.setMatrixAt(slot, _scratchMatrix);
    batch.mesh.instanceMatrix.addUpdateRange(slot * 16, 16);
    batch.mesh.instanceMatrix.needsUpdate = true;
  }
}

function createSlotArray(capacity: number): Int32Array {
  const a = new Int32Array(capacity);
  a.fill(-1);
  return a;
}

/**
 * Allocate the per-instance color attribute synchronously and prefill all
 * slots to white. Three.js triggers a shader recompile on the first non-null
 * `instanceColor` write (#21786) — doing this once at mesh creation moves
 * the cost out of the hot path.
 */
function allocateInstanceColor(mesh: THREE.InstancedMesh, capacity: number): void {
  const data = new Float32Array(capacity * 3);
  data.fill(1);
  const attr = new THREE.InstancedBufferAttribute(data, 3);
  attr.setUsage(THREE.DynamicDrawUsage);
  mesh.instanceColor = attr;
}
