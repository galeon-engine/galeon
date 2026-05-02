// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { InstancedMesh2, type BVHParams } from "@three.ez/instanced-mesh";
import * as THREE from "three";
import { TRANSFORM_STRIDE } from "@galeon/render-core";

/** Symbol key for resolving instanced hit `instanceId`s to Galeon entities. */
export const GALEON_INSTANCE_ENTITIES_KEY: unique symbol = Symbol.for(
  "galeon.instanceEntities",
);

export interface InstancedEntityRef {
  readonly entityId: number;
  readonly generation: number;
}

export interface InstancedEntityResolver {
  entityAt(instanceId: number): InstancedEntityRef | undefined;
}

export type GaleonInstancedMesh = InstancedMesh2<unknown, THREE.BufferGeometry, THREE.Material>;

export interface InstancedEntityPlacement {
  readonly mesh: GaleonInstancedMesh;
  readonly instanceId: number;
}

export interface InstancedMeshManagerOptions {
  /**
   * Optional renderer passed to `InstancedMesh2` so GPU buffers can initialize
   * before the first render. Headless tests and tools may omit it.
   */
  readonly renderer?: THREE.WebGLRenderer;
  /** BVH construction options for Galeon's instanced batches. */
  readonly bvh?: BVHParams;
}

/** Stable key that identifies one instanced batch. */
export type InstanceBatchKey = number | bigint;

/** Initial slot capacity allocated when a new instance batch is created. */
const INITIAL_CAPACITY = 16;

interface Batch {
  readonly groupKey: InstanceBatchKey;
  mesh: GaleonInstancedMesh;
  /** `slot -> entityId` (`-1` for empty / removed slots). */
  slotToEntity: Int32Array;
  /** `slot -> entity generation`, parallel to `slotToEntity`. */
  slotToGeneration: Uint32Array;
  /** Last registry-resolved geometry and material for this batch. */
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
}

const _scratchPos = new THREE.Vector3();
const _scratchQuat = new THREE.Quaternion();
const _scratchScale = new THREE.Vector3();
const _scratchMatrix = new THREE.Matrix4();
const _scratchColor = new THREE.Color();
const _white = new THREE.Color(1, 1, 1);

export class InstancedMeshManager {
  private readonly scene: THREE.Scene;
  private readonly renderer: THREE.WebGLRenderer | undefined;
  private readonly bvhOptions: BVHParams;
  private readonly batches = new Map<InstanceBatchKey, Batch>();
  /** `entityId -> (groupKey, slot)`; single source of truth for placement. */
  private readonly entityToPlacement = new Map<
    number,
    { groupKey: InstanceBatchKey; slot: number }
  >();

  constructor(scene: THREE.Scene, options: InstancedMeshManagerOptions = {}) {
    this.scene = scene;
    this.renderer = options.renderer;
    this.bvhOptions = options.bvh ?? { margin: 0 };
  }

  // ---------------------------------------------------------------------------
  // Mutations
  // ---------------------------------------------------------------------------

  /**
   * Insert or update an entity inside its BVH-backed instance batch.
   *
   * Migrations (entity already placed in a different batch) remove from the old
   * batch first. Slots are stable while an entity remains in its batch; removed
   * slots are returned to `InstancedMesh2`'s free list and may be reused later.
   */
  upsert(
    entityId: number,
    generation: number,
    groupKey: InstanceBatchKey,
    geometry: THREE.BufferGeometry,
    material: THREE.Material,
    transforms: Float32Array,
    transformIndex: number,
    visible: boolean,
    tints?: Float32Array,
  ): { groupKey: InstanceBatchKey; slot: number } {
    const existing = this.entityToPlacement.get(entityId);
    if (existing !== undefined && existing.groupKey !== groupKey) {
      this.remove(entityId);
    }

    const batch = this.getOrCreateBatch(groupKey, geometry, material);
    let placement = this.entityToPlacement.get(entityId);
    if (placement === undefined) {
      const slot = this.allocateSlot(batch);
      batch.slotToEntity[slot] = entityId;
      batch.slotToGeneration[slot] = generation;
      placement = { groupKey, slot };
      this.entityToPlacement.set(entityId, placement);
    } else {
      batch.slotToGeneration[placement.slot] = generation;
    }

    this.writeSlotMatrix(batch, placement.slot, transforms, transformIndex, visible);
    if (tints !== undefined) {
      this.writeSlotTint(batch, placement.slot, tints, transformIndex);
    }
    return { groupKey, slot: placement.slot };
  }

  /**
   * Remove an entity from its instance batch, if any. Returns whether the
   * entity was actually placed.
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
    batch.mesh.removeInstances(slot);
    batch.slotToEntity[slot] = -1;
    batch.slotToGeneration[slot] = 0;
    this.entityToPlacement.delete(entityId);
    return true;
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  /** The instanced mesh backing a group key, if one has been created. */
  meshFor(groupKey: InstanceBatchKey): GaleonInstancedMesh | undefined {
    return this.batchForQuery(groupKey)?.mesh;
  }

  /** Number of live instance slots in a group's batch. */
  slotsFor(groupKey: InstanceBatchKey): number {
    return this.batchesForQuery(groupKey).reduce(
      (count, batch) => count + batch.mesh.instancesCount,
      0,
    );
  }

  /** Allocated capacity of a group's batch; distinct from live slot count. */
  capacityFor(groupKey: InstanceBatchKey): number {
    return this.batchesForQuery(groupKey).reduce(
      (capacity, batch) => capacity + batch.mesh.capacity,
      0,
    );
  }

  /** Number of distinct mesh/material batches currently allocated. */
  get batchCount(): number {
    return this.batches.size;
  }

  /** Whether this entity is currently placed in any instance batch. */
  has(entityId: number): boolean {
    return this.entityToPlacement.has(entityId);
  }

  /** Resolve an entity to its current instanced mesh slot, if it is batched. */
  getEntityInstance(
    entityId: number,
    generation: number,
  ): InstancedEntityPlacement | undefined {
    const placement = this.entityToPlacement.get(entityId);
    if (placement === undefined) return undefined;
    const batch = this.batches.get(placement.groupKey);
    if (batch === undefined) return undefined;
    if (batch.slotToGeneration[placement.slot] !== generation) return undefined;
    if (!batch.mesh.getActiveAt(placement.slot)) return undefined;
    return {
      mesh: batch.mesh,
      instanceId: placement.slot,
    };
  }

  /** Iterate over every entity currently held in any batch. */
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

  /** Alias for `clear()` so callers can mirror `RendererCache.dispose()`. */
  dispose(): void {
    this.clear();
  }

  // ---------------------------------------------------------------------------
  // Internal
  // ---------------------------------------------------------------------------

  private getOrCreateBatch(
    groupKey: InstanceBatchKey,
    geometry: THREE.BufferGeometry,
    material: THREE.Material,
  ): Batch {
    let batch = this.batches.get(groupKey);
    if (batch !== undefined) {
      if (batch.geometry !== geometry) {
        batch.mesh.geometry = geometry;
        batch.geometry = geometry;
        batch.mesh.disposeBVH();
        batch.mesh.computeBVH(this.bvhOptions);
      }
      if (batch.material !== material) {
        batch.mesh.material = material;
        batch.material = material;
      }
      return batch;
    }

    const mesh = new InstancedMesh2(geometry, material, {
      capacity: INITIAL_CAPACITY,
      renderer: this.renderer,
    }) as GaleonInstancedMesh;
    mesh.frustumCulled = false;
    mesh.computeBVH(this.bvhOptions);
    this.scene.add(mesh);

    batch = {
      groupKey,
      mesh,
      slotToEntity: createSlotArray(mesh.capacity),
      slotToGeneration: new Uint32Array(mesh.capacity),
      geometry,
      material,
    };
    attachResolver(mesh, batch);
    this.batches.set(groupKey, batch);
    return batch;
  }

  private allocateSlot(batch: Batch): number {
    let slot = -1;
    batch.mesh.addInstances(1, (_, index) => {
      slot = index;
    });
    if (slot < 0) {
      throw new Error("InstancedMesh2 failed to allocate an instance slot");
    }
    ensureSlotCapacity(batch, batch.mesh.capacity);
    batch.mesh.setColorAt(slot, _white);
    return slot;
  }

  private batchForQuery(groupKey: InstanceBatchKey): Batch | undefined {
    const direct = this.batches.get(groupKey);
    if (direct !== undefined) return direct;

    const matches = this.batchesForQuery(groupKey);
    return matches.length === 1 ? matches[0] : undefined;
  }

  private batchesForQuery(groupKey: InstanceBatchKey): Batch[] {
    const direct = this.batches.get(groupKey);
    if (direct !== undefined) return [direct];
    if (typeof groupKey === "bigint") return [];

    const prefix = BigInt(groupKey) << 32n;
    const nextPrefix = BigInt(groupKey + 1) << 32n;
    const matches: Batch[] = [];
    for (const [key, batch] of this.batches) {
      if (typeof key === "bigint" && key >= prefix && key < nextPrefix) {
        matches.push(batch);
      }
    }
    return matches;
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
    _scratchScale.set(
      transforms[off + 7]!,
      transforms[off + 8]!,
      transforms[off + 9]!,
    );
    _scratchMatrix.compose(_scratchPos, _scratchQuat, _scratchScale);
    batch.mesh.setMatrixAt(slot, _scratchMatrix);
    batch.mesh.setVisibilityAt(slot, visible);
  }
}

function createSlotArray(capacity: number): Int32Array {
  const a = new Int32Array(capacity);
  a.fill(-1);
  return a;
}

function ensureSlotCapacity(batch: Batch, capacity: number): void {
  if (batch.slotToEntity.length >= capacity) return;
  const nextEntities = createSlotArray(capacity);
  nextEntities.set(batch.slotToEntity);
  const nextGenerations = new Uint32Array(capacity);
  nextGenerations.set(batch.slotToGeneration);
  batch.slotToEntity = nextEntities;
  batch.slotToGeneration = nextGenerations;
}

function attachResolver(mesh: GaleonInstancedMesh, batch: Batch): void {
  const resolver: InstancedEntityResolver = {
    entityAt(instanceId: number): InstancedEntityRef | undefined {
      if (!Number.isInteger(instanceId) || instanceId < 0) return undefined;
      if (instanceId >= batch.slotToEntity.length) return undefined;
      if (!batch.mesh.getActiveAt(instanceId)) return undefined;
      const entityId = batch.slotToEntity[instanceId]!;
      if (entityId < 0) return undefined;
      return {
        entityId,
        generation: batch.slotToGeneration[instanceId]!,
      };
    },
  };
  (mesh.userData as Record<PropertyKey, unknown>)[GALEON_INSTANCE_ENTITIES_KEY] = resolver;
}
