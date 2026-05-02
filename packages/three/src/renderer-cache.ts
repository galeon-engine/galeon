// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import {
  CHANGED_MATERIAL,
  CHANGED_MESH,
  CHANGED_PARENT,
  CHANGED_TRANSFORM,
  CHANGED_VISIBILITY,
  INSTANCE_GROUP_NONE,
  assertFramePacketContract,
  type FramePacketView,
  ObjectType,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
} from "@galeon/render-core";
import {
  InstancedMeshManager,
  type InstanceBatchKey,
} from "./instanced-mesh-manager.js";
import { isBillboardFbmMaterial } from "./materials/billboard-fbm.js";

/**
 * Renderer-side cache that consumes packed extraction tables from the
 * WASM engine and applies bulk updates to a Three.js scene graph.
 *
 * Per frame:
 * 1. Iterates the FramePacket arrays
 * 2. Creates Three.js objects for new entities
 * 3. Updates transform, visibility, mesh, and material for existing entities
 * 4. Removes objects for entities no longer present
 *
 * Geometry and material are resolved via user-provided registries. If no
 * registry entry exists for a handle, a placeholder is used.
 */
/**
 * Symbol key for the entity back-pointer stamped on every managed Three.js object's `userData`.
 * Using a Symbol avoids namespace collisions with string-keyed custom render channels.
 */
export const GALEON_ENTITY_KEY: unique symbol = Symbol.for("galeon.entity");
const INSTANCE_KEY_SHIFT = 32n;
const INSTANCE_KEY_MASK = 0xffff_ffffn;

export interface RendererEntityHandle {
  readonly entityId: number;
  generation: number;
  object: THREE.Object3D;
}

export class RendererCache {
  private readonly scene: THREE.Scene;
  private readonly instancedMeshes: InstancedMeshManager;
  private readonly objects = new Map<number, THREE.Object3D>();
  private readonly generations = new Map<number, number>();
  private readonly entityHandles = new Map<number, RendererEntityHandle>();
  private readonly geometries = new Map<number, THREE.BufferGeometry>();
  private readonly materials = new Map<number, THREE.Material>();

  /** Last registry-resolved geometry per entity (not obj.geometry — that may be consumer-overridden). */
  private readonly resolvedGeometries = new Map<number, THREE.BufferGeometry>();
  /** Last registry-resolved material per entity (not obj.material — that may be consumer-overridden). */
  private readonly resolvedMaterials = new Map<number, THREE.Material>();

  /** Current parent entity index per entity (`SCENE_ROOT` = scene root). */
  private readonly parentOf = new Map<number, number>();
  /**
   * Child -> previous parent mapping for standalone children orphaned when the
   * parent is temporarily routed into instancing.
   */
  private readonly detachedChildrenParentHint = new Map<number, number>();

  /** Entities already warned about missing mesh handles (cleared when handle becomes registered). */
  private readonly warnedMeshes = new Set<number>();
  /** Entities already warned about missing material handles (cleared when handle becomes registered). */
  private readonly warnedMaterials = new Set<number>();

  /** Last applied `ObjectType` discriminant per entity (matches packet `object_types` / Rust repr). */
  private readonly objectKinds = new Map<number, number>();

  /** Frame version from the last applied packet (demand rendering). */
  private lastFrameVersion: bigint = 0n;
  /** Whether the last `applyFrame()` made any scene changes. */
  private _dirty = false;

  /** Fallback geometry used when a mesh handle has no registered geometry. */
  private readonly placeholderGeometry = new THREE.BoxGeometry(1, 1, 1);
  /** Fallback material used when a material handle has no registered material. */
  private readonly placeholderMaterial = new THREE.MeshBasicMaterial({
    color: 0xff00ff,
    wireframe: true,
  });

  /**
   * Called when an entity is about to be removed from the cache (despawn,
   * stale-generation eviction, or `clear()`). The mesh has already been
   * removed from the scene but not yet deleted from internal maps.
   *
   * Use this to dispose consumer-owned GPU resources (geometry, material)
   * that the cache cannot safely auto-dispose because it does not own them.
   */
  onEntityRemoved?: (entityId: number, generation: number, obj: THREE.Object3D) => void;

  constructor(scene: THREE.Scene) {
    this.scene = scene;
    this.instancedMeshes = new InstancedMeshManager(scene);
  }

  /**
   * The instance-batch manager backing entities tagged with `InstanceOf`.
   * Exposed read-only so adapter layers can query batch state for telemetry.
   */
  get instancing(): InstancedMeshManager {
    return this.instancedMeshes;
  }

  // ---------------------------------------------------------------------------
  // Asset registration
  // ---------------------------------------------------------------------------

  /** Register a geometry for a mesh handle ID. Invalidates cached frame version so the next `applyFrame()` re-resolves handles. */
  registerGeometry(id: number, geometry: THREE.BufferGeometry): void {
    this.geometries.set(id, geometry);
    this.lastFrameVersion = 0n;
  }

  /** Register a material for a material handle ID. Invalidates cached frame version so the next `applyFrame()` re-resolves handles. */
  registerMaterial(id: number, material: THREE.Material): void {
    this.materials.set(id, material);
    this.lastFrameVersion = 0n;
  }

  // ---------------------------------------------------------------------------
  // Frame application
  // ---------------------------------------------------------------------------

  /**
   * Apply a frame packet to the scene graph.
   *
   * Call once per render frame after `WasmEngine.extract_frame()`.
   */
  applyFrame(packet: FramePacketView): void {
    assertFramePacketContract(packet);

    if (packet.frame_version != null && packet.frame_version === this.lastFrameVersion) {
      this._dirty = false;
      return;
    }
    if (packet.frame_version != null) {
      this.lastFrameVersion = packet.frame_version;
    }
    this._dirty = true;

    const activeIds = new Set<number>();
    const customChannels = Array.from({ length: packet.custom_channel_count }, (_, index) => {
      const name = packet.custom_channel_name_at(index);
      return {
        name,
        stride: packet.custom_channel_stride(name),
        data: packet.custom_channel_data(name),
      };
    });
    const {
      entity_ids,
      entity_generations,
      transforms,
      visibility,
      mesh_handles,
      material_handles,
      parent_ids,
    } = packet;
    const changeFlags = packet.change_flags;
    const hasChangeFlags = changeFlags !== undefined && changeFlags.length > 0;
    const instanceGroups = packet.instance_groups;
    const tints = packet.tints;
    // Entities that exited the instanced path this frame and need parent
    // reconciliation even when CHANGED_PARENT is not set.
    const forceParentReconcile = new Set<number>();

    // ----- Pass 1: create/update objects -----
    for (let i = 0; i < packet.entity_count; i++) {
      // Typed arrays are bounds-controlled by entity_count; non-null asserts are safe here.
      const entityId = entity_ids[i]!;
      const generation = entity_generations[i]!;
      activeIds.add(entityId);
      const flag = hasChangeFlags ? changeFlags[i]! : 0xff;

      // ----- Instanced routing -----
      // Entities tagged with `InstanceOf` skip the standalone-Object3D path
      // and live in `THREE.InstancedMesh` batches instead.
      // Default batch key is the wrapped `MeshHandle.id`; billboard materials
      // are split by `(instanceGroup, materialHandle)` to keep per-texture /
      // per-material variants in separate batches.
      const groupKey = instanceGroups?.[i] ?? INSTANCE_GROUP_NONE;
      if (groupKey !== INSTANCE_GROUP_NONE) {
        // If the entity was previously standalone, tear it down before
        // routing into the batch — the entity must not own both objects.
        const stale = this.objects.get(entityId);
        if (stale) {
          this.removeEntity(entityId, stale, { rememberChildrenForReattach: true });
        }

        const meshHandle = mesh_handles[i]!;
        const matHandle = material_handles[i]!;
        const geometry =
          this.geometries.get(meshHandle) ?? this.placeholderGeometry;
        const material =
          this.materials.get(matHandle) ?? this.placeholderMaterial;
        const visible = visibility[i]! === 1;
        const batchKey = this.resolveInstanceBatchKey(
          groupKey,
          matHandle,
          material,
        );
        this.instancedMeshes.upsert(
          entityId,
          batchKey,
          geometry,
          material,
          transforms,
          i,
          visible,
          tints,
        );
        // Track generation for stale-slot detection across despawn/respawn.
        this.generations.set(entityId, generation);
        this.warnMissingHandles(entityId, meshHandle, matHandle);
        continue;
      }

      // If the entity was previously instanced and now wants the standalone
      // path, drop it from the batch before falling through to creation.
      if (this.instancedMeshes.has(entityId)) {
        this.instancedMeshes.remove(entityId);
        forceParentReconcile.add(entityId);
      }

      let obj = this.objects.get(entityId);
      let childrenToReattach: number[] | undefined;

      // If the slot was reused (generation mismatch), remove the stale object
      // so we create a fresh one below. This prevents the stale-entity bug
      // that generational IDs are designed to catch.
      if (obj && this.generations.get(entityId) !== generation) {
        this.removeEntity(entityId, obj);
        obj = undefined;
      }

      if (obj) {
        const ot = packet.object_types;
        const desiredKind = ot !== undefined && i < ot.length ? ot[i]! : undefined;
        const effectiveStored = this.objectKinds.get(entityId) ?? ObjectType.Mesh;
        if (desiredKind !== undefined && desiredKind !== effectiveStored) {
          childrenToReattach = Array.from(this.parentOf.entries())
            .filter(([, parentId]) => parentId === entityId)
            .map(([childId]) => childId);
          this.removeEntity(entityId, obj);
          obj = undefined;
        }
      }

      if (!obj) {
        const meshHandle = mesh_handles[i]!;
        const matHandle = material_handles[i]!;
        const objectType = packet.object_types?.[i] ?? ObjectType.Mesh;
        const needsGeoMat = RendererCache.needsGeometryMaterial(objectType);
        const geometry = needsGeoMat
          ? (this.geometries.get(meshHandle) ?? this.placeholderGeometry)
          : this.placeholderGeometry;
        const material = needsGeoMat
          ? (this.materials.get(matHandle) ?? this.placeholderMaterial)
          : this.placeholderMaterial;
        obj = this.createObject(objectType, geometry, material);
        obj.matrixAutoUpdate = false;
        this.objects.set(entityId, obj);
        this.generations.set(entityId, generation);
        if (needsGeoMat) {
          this.resolvedGeometries.set(entityId, geometry);
          this.resolvedMaterials.set(entityId, material);
          this.warnMissingHandles(entityId, meshHandle, matHandle);
        }
        (obj.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = { entityId, generation };
        // Temporarily add to scene; Pass 2 will reparent if needed.
        this.scene.add(obj);
        this.parentOf.set(entityId, SCENE_ROOT);
        this.objectKinds.set(entityId, packet.object_types?.[i] ?? ObjectType.Mesh);
        this.entityHandles.set(entityId, { entityId, generation, object: obj });
        if (childrenToReattach !== undefined) {
          for (const childId of childrenToReattach) {
            const childObj = this.objects.get(childId);
            if (!childObj) continue;
            childObj.parent?.remove(childObj);
            obj.add(childObj);
            this.parentOf.set(childId, entityId);
          }
        }
        this.applyTransform(obj, i, transforms);
        obj.visible = visibility[i]! === 1;
      } else {
        const meshOrMat = flag & (CHANGED_MESH | CHANGED_MATERIAL);
        if (meshOrMat !== 0 && obj instanceof THREE.Mesh) {
          // Compare the registry resolution against what we last resolved — NOT
          // against obj.geometry/obj.material (which may be consumer-overridden).
          // This lets consumer overrides survive while still picking up late
          // registrations and same-handle rebinds.
          const meshHandle = mesh_handles[i]!;
          const matHandle = material_handles[i]!;
          const geometry = this.geometries.get(meshHandle) ?? this.placeholderGeometry;
          const material = this.materials.get(matHandle) ?? this.placeholderMaterial;
          if ((flag & CHANGED_MESH) !== 0 && this.resolvedGeometries.get(entityId) !== geometry) {
            obj.geometry = geometry;
            this.resolvedGeometries.set(entityId, geometry);
          }
          if ((flag & CHANGED_MATERIAL) !== 0 && this.resolvedMaterials.get(entityId) !== material) {
            obj.material = material;
            this.resolvedMaterials.set(entityId, material);
          }
          this.warnMissingHandles(entityId, meshHandle, matHandle);
        }
        if ((flag & CHANGED_TRANSFORM) !== 0) {
          this.applyTransform(obj, i, transforms);
        }
        if ((flag & CHANGED_VISIBILITY) !== 0) {
          obj.visible = visibility[i]! === 1;
        }
      }

      if (forceParentReconcile.has(entityId)) {
        this.restoreDetachedChildren(entityId, obj);
      }

      // Custom channels: always applied per entity when present on the packet.
      // There is no CHANGED_CUSTOM (or per-channel) bit yet, so change_flags do not
      // gate this path — a future optimization could skip writes when the Rust
      // extractor can signal unchanged channel rows.
      for (const channel of customChannels) {
        const { name, stride, data } = channel;
        const off = i * stride;
        if (stride === 1) {
          obj.userData[name] = data[off]!;
        } else {
          obj.userData[name] = data.slice(off, off + stride);
        }
      }
    }

    // ----- Pass 2: reparent objects based on parent_ids -----
    // Entities arrive depth-sorted (parent before child) from the Rust
    // extraction, so a forward pass correctly builds the hierarchy.
    for (let i = 0; i < packet.entity_count; i++) {
      const entityId = entity_ids[i]!;
      const flag = hasChangeFlags ? changeFlags[i]! : 0xff;
      const shouldReparent =
        (flag & CHANGED_PARENT) !== 0 || forceParentReconcile.has(entityId);
      if (!shouldReparent) continue;

      // Explicit parent reconciliation supersedes stale migration hints.
      this.detachedChildrenParentHint.delete(entityId);

      const parentId = parent_ids[i]!;
      const prevParent = this.parentOf.get(entityId);
      if (prevParent === parentId) continue;

      const obj = this.objects.get(entityId);
      if (!obj) continue;

      if (parentId === SCENE_ROOT) {
        if (obj.parent !== this.scene) {
          obj.parent?.remove(obj);
          this.scene.add(obj);
        }
      } else {
        const parentObj = this.objects.get(parentId);
        if (parentObj && obj.parent !== parentObj) {
          obj.parent?.remove(obj);
          parentObj.add(obj);
        }
      }
      this.parentOf.set(entityId, parentId);
    }

    // Remove objects for entities that disappeared this frame.
    // Skip for incremental packets — they only include changed entities,
    // so absence does NOT mean despawned.
    if (!hasChangeFlags) {
      for (const [id, obj] of this.objects) {
        if (!activeIds.has(id)) {
          this.removeEntity(id, obj);
        }
      }
      // Same rule for the instance batches — collect first so the
      // remove() loop doesn't mutate the iteration.
      const instancedToRemove: number[] = [];
      for (const id of this.instancedMeshes.entityIds()) {
        if (!activeIds.has(id)) instancedToRemove.push(id);
      }
      for (const id of instancedToRemove) {
        this.instancedMeshes.remove(id);
        this.generations.delete(id);
        this.warnedMeshes.delete(id);
        this.warnedMaterials.delete(id);
        this.clearDetachedChildrenHintsForParent(id);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  /** Whether the last `applyFrame()` made any scene changes. */
  get needsRender(): boolean {
    return this._dirty;
  }

  /** Number of active Three.js objects managed by this cache. */
  get objectCount(): number {
    return this.objects.size;
  }

  /**
   * Get the Three.js object for an entity, if it exists and the generation
   * matches. Returns `undefined` if the entity was despawned and the slot
   * reused — preserving generational safety through the public API.
   */
  getObject(entityId: number, generation: number): THREE.Object3D | undefined {
    if (this.generations.get(entityId) !== generation) return undefined;
    return this.objects.get(entityId);
  }

  /**
   * Stable entity handles for adapter layers that need identity-preserving
   * references tied to Three.js objects.
   */
  listEntityHandles(): readonly RendererEntityHandle[] {
    return Array.from(this.entityHandles.values());
  }

  /**
   * Look up a stable entity handle by `(entityId, generation)`.
   */
  getEntityHandle(entityId: number, generation: number): RendererEntityHandle | undefined {
    if (this.generations.get(entityId) !== generation) return undefined;
    return this.entityHandles.get(entityId);
  }

  // ---------------------------------------------------------------------------
  // Cleanup
  // ---------------------------------------------------------------------------

  /** Remove all objects from the scene, notify via `onEntityRemoved`, and clear the cache. */
  clear(): void {
    for (const [id, obj] of this.objects) {
      this.removeEntity(id, obj);
    }
    this.instancedMeshes.clear();
    this.detachedChildrenParentHint.clear();
    this.lastFrameVersion = 0n;
    this._dirty = true;
  }

  /** Dispose of placeholder resources. Call when the cache is no longer needed. */
  dispose(): void {
    this.clear();
    this.instancedMeshes.dispose();
    this.placeholderGeometry.dispose();
    this.placeholderMaterial.dispose();
  }

  // ---------------------------------------------------------------------------
  // Internal
  // ---------------------------------------------------------------------------

  /**
   * Returns true if this object type uses geometry and material.
   * Lights and Groups don't need them — skip registry resolution and warnings.
   */
  private static needsGeometryMaterial(objectType: number): boolean {
    return objectType === ObjectType.Mesh || objectType === ObjectType.LineSegments;
  }

  private createObject(
    objectType: number,
    geometry: THREE.BufferGeometry,
    material: THREE.Material,
  ): THREE.Object3D {
    switch (objectType) {
      case ObjectType.PointLight:
        return new THREE.PointLight();
      case ObjectType.DirectionalLight:
        return new THREE.DirectionalLight();
      case ObjectType.LineSegments:
        return new THREE.LineSegments(geometry, material);
      case ObjectType.Group:
        return new THREE.Group();
      default:
        return new THREE.Mesh(geometry, material);
    }
  }

  /**
   * Remove an entity's mesh from the scene and clean up internal tracking.
   * Notifies `onEntityRemoved` so the consumer can dispose resources it owns.
   */
  private restoreDetachedChildren(parentId: number, parentObj: THREE.Object3D): void {
    const childIdsToRestore = new Set<number>();
    for (const [childId, hintedParentId] of this.detachedChildrenParentHint) {
      if (hintedParentId === parentId) childIdsToRestore.add(childId);
    }
    for (const [childId, currentParentId] of this.parentOf) {
      if (currentParentId === parentId) childIdsToRestore.add(childId);
    }
    if (childIdsToRestore.size === 0) return;

    for (const childId of childIdsToRestore) {
      const childObj = this.objects.get(childId);
      if (!childObj) continue;
      const currentParentId = this.parentOf.get(childId);
      if (currentParentId !== SCENE_ROOT && currentParentId !== parentId) continue;
      if (childObj.parent !== parentObj) {
        childObj.parent?.remove(childObj);
        parentObj.add(childObj);
      }
      this.parentOf.set(childId, parentId);
    }
    for (const childId of childIdsToRestore) {
      this.detachedChildrenParentHint.delete(childId);
    }
  }

  private clearDetachedChildrenHintsForParent(parentId: number): void {
    for (const [childId, hintedParentId] of this.detachedChildrenParentHint) {
      if (hintedParentId === parentId) {
        this.detachedChildrenParentHint.delete(childId);
      }
    }
  }

  private removeEntity(
    id: number,
    obj: THREE.Object3D,
    options: { rememberChildrenForReattach?: boolean } = {},
  ): void {
    // Reparent orphaned children to the scene root before detaching.
    // Without this, children become invisible (still attached to a
    // removed parent object that is no longer in the scene graph).
    for (const [childId, parentId] of this.parentOf) {
      if (parentId !== id) continue;
      const childObj = this.objects.get(childId);
      if (childObj) {
        obj.remove(childObj);
        this.scene.add(childObj);
        if (options.rememberChildrenForReattach) {
          this.detachedChildrenParentHint.set(childId, id);
        }
      }
      this.parentOf.set(childId, SCENE_ROOT);
    }

    // Detach from whatever parent (scene or another object).
    obj.parent?.remove(obj);
    try {
      this.onEntityRemoved?.(id, this.generations.get(id)!, obj);
    } finally {
      this.objects.delete(id);
      this.generations.delete(id);
      this.resolvedGeometries.delete(id);
      this.resolvedMaterials.delete(id);
      this.entityHandles.delete(id);
      this.parentOf.delete(id);
      this.warnedMeshes.delete(id);
      this.warnedMaterials.delete(id);
      this.objectKinds.delete(id);
      this.detachedChildrenParentHint.delete(id);
    }
  }

  private applyTransform(obj: THREE.Object3D, i: number, transforms: Float32Array): void {
    const off = i * TRANSFORM_STRIDE;
    obj.position.set(transforms[off]!, transforms[off + 1]!, transforms[off + 2]!);
    obj.quaternion.set(
      transforms[off + 3]!,
      transforms[off + 4]!,
      transforms[off + 5]!,
      transforms[off + 6]!,
    );
    obj.scale.set(transforms[off + 7]!, transforms[off + 8]!, transforms[off + 9]!);
    obj.updateMatrix();
  }

  private warnMissingHandles(entityId: number, meshHandle: number, matHandle: number): void {
    if (!this.geometries.has(meshHandle)) {
      if (!this.warnedMeshes.has(entityId)) {
        console.warn(`[RendererCache] No geometry registered for mesh handle ${meshHandle} (entity ${entityId})`);
        this.warnedMeshes.add(entityId);
      }
    } else {
      this.warnedMeshes.delete(entityId);
    }
    if (!this.materials.has(matHandle)) {
      if (!this.warnedMaterials.has(entityId)) {
        console.warn(`[RendererCache] No material registered for handle ${matHandle} (entity ${entityId})`);
        this.warnedMaterials.add(entityId);
      }
    } else {
      this.warnedMaterials.delete(entityId);
    }
  }

  /**
   * Instancing key policy:
   * - General instancing: batch by Rust `InstanceOf(MeshHandle)` group id.
   * - Billboard FBM materials: batch by `(instanceGroup, materialHandle)` so
   *   different texture/material variants do not thrash one shared batch.
   */
  private resolveInstanceBatchKey(
    instanceGroup: number,
    materialHandle: number,
    material: THREE.Material,
  ): InstanceBatchKey {
    if (!isBillboardFbmMaterial(material)) {
      return instanceGroup;
    }
    return (
      (BigInt(instanceGroup) << INSTANCE_KEY_SHIFT) |
      (BigInt(materialHandle) & INSTANCE_KEY_MASK)
    );
  }
}
