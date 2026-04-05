// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import { type FramePacketView, TRANSFORM_STRIDE } from "./types.js";

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
export class RendererCache {
  private readonly scene: THREE.Scene;
  private readonly objects = new Map<number, THREE.Mesh>();
  private readonly generations = new Map<number, number>();
  private readonly geometries = new Map<number, THREE.BufferGeometry>();
  private readonly materials = new Map<number, THREE.Material>();

  /** Last registry-resolved geometry per entity (not obj.geometry — that may be consumer-overridden). */
  private readonly resolvedGeometries = new Map<number, THREE.BufferGeometry>();
  /** Last registry-resolved material per entity (not obj.material — that may be consumer-overridden). */
  private readonly resolvedMaterials = new Map<number, THREE.Material>();

  /** Entities already warned about missing mesh handles (cleared when handle becomes registered). */
  private readonly warnedMeshes = new Set<number>();
  /** Entities already warned about missing material handles (cleared when handle becomes registered). */
  private readonly warnedMaterials = new Set<number>();

  /** Fallback geometry used when a mesh handle has no registered geometry. */
  private readonly placeholderGeometry = new THREE.BoxGeometry(1, 1, 1);
  /** Fallback material used when a material handle has no registered material. */
  private readonly placeholderMaterial = new THREE.MeshBasicMaterial({
    color: 0xff00ff,
    wireframe: true,
  });

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  // ---------------------------------------------------------------------------
  // Asset registration
  // ---------------------------------------------------------------------------

  /** Register a geometry for a mesh handle ID. */
  registerGeometry(id: number, geometry: THREE.BufferGeometry): void {
    this.geometries.set(id, geometry);
  }

  /** Register a material for a material handle ID. */
  registerMaterial(id: number, material: THREE.Material): void {
    this.materials.set(id, material);
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
    } = packet;

    for (let i = 0; i < packet.entity_count; i++) {
      // Typed arrays are bounds-controlled by entity_count; non-null asserts are safe here.
      const entityId = entity_ids[i]!;
      const generation = entity_generations[i]!;
      activeIds.add(entityId);

      let obj = this.objects.get(entityId);

      // If the slot was reused (generation mismatch), remove the stale object
      // so we create a fresh one below. This prevents the stale-entity bug
      // that generational IDs are designed to catch.
      if (obj && this.generations.get(entityId) !== generation) {
        this.scene.remove(obj);
        this.objects.delete(entityId);
        this.resolvedGeometries.delete(entityId);
        this.resolvedMaterials.delete(entityId);
        this.warnedMeshes.delete(entityId);
        this.warnedMaterials.delete(entityId);
        obj = undefined;
      }

      if (!obj) {
        const meshHandle = mesh_handles[i]!;
        const matHandle = material_handles[i]!;
        const geometry = this.geometries.get(meshHandle) ?? this.placeholderGeometry;
        const material = this.materials.get(matHandle) ?? this.placeholderMaterial;
        obj = new THREE.Mesh(geometry, material);
        obj.matrixAutoUpdate = false;
        this.objects.set(entityId, obj);
        this.generations.set(entityId, generation);
        this.resolvedGeometries.set(entityId, geometry);
        this.resolvedMaterials.set(entityId, material);
        this.warnMissingHandles(entityId, meshHandle, matHandle);
        this.scene.add(obj);
      } else {
        // Compare the registry resolution against what we last resolved — NOT
        // against obj.geometry/obj.material (which may be consumer-overridden).
        // This lets consumer overrides survive while still picking up late
        // registrations and same-handle rebinds.
        const meshHandle = mesh_handles[i]!;
        const matHandle = material_handles[i]!;
        const geometry = this.geometries.get(meshHandle) ?? this.placeholderGeometry;
        const material = this.materials.get(matHandle) ?? this.placeholderMaterial;
        if (this.resolvedGeometries.get(entityId) !== geometry) {
          obj.geometry = geometry;
          this.resolvedGeometries.set(entityId, geometry);
        }
        if (this.resolvedMaterials.get(entityId) !== material) {
          obj.material = material;
          this.resolvedMaterials.set(entityId, material);
        }
        this.warnMissingHandles(entityId, meshHandle, matHandle);
      }

      // Update transform — read 10 floats at offset i * TRANSFORM_STRIDE.
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

      // Update visibility.
      obj.visible = visibility[i]! === 1;

      // Apply custom channel data to userData.
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

    // Remove objects for entities that disappeared this frame.
    for (const [id, obj] of this.objects) {
      if (!activeIds.has(id)) {
        this.scene.remove(obj);
        this.objects.delete(id);
        this.generations.delete(id);
        this.resolvedGeometries.delete(id);
        this.resolvedMaterials.delete(id);
        this.warnedMeshes.delete(id);
        this.warnedMaterials.delete(id);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  /** Number of active Three.js objects managed by this cache. */
  get objectCount(): number {
    return this.objects.size;
  }

  /**
   * Get the Three.js object for an entity, if it exists and the generation
   * matches. Returns `undefined` if the entity was despawned and the slot
   * reused — preserving generational safety through the public API.
   */
  getObject(entityId: number, generation: number): THREE.Mesh | undefined {
    if (this.generations.get(entityId) !== generation) return undefined;
    return this.objects.get(entityId);
  }

  // ---------------------------------------------------------------------------
  // Cleanup
  // ---------------------------------------------------------------------------

  /** Remove all objects from the scene and clear the cache. */
  clear(): void {
    for (const obj of this.objects.values()) {
      this.scene.remove(obj);
    }
    this.objects.clear();
    this.generations.clear();
    this.resolvedGeometries.clear();
    this.resolvedMaterials.clear();
    this.warnedMeshes.clear();
    this.warnedMaterials.clear();
  }

  /** Dispose of placeholder resources. Call when the cache is no longer needed. */
  dispose(): void {
    this.clear();
    this.placeholderGeometry.dispose();
    this.placeholderMaterial.dispose();
  }

  // ---------------------------------------------------------------------------
  // Internal
  // ---------------------------------------------------------------------------

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
}
