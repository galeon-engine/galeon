// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import {
  ObjectType,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  hasIncrementalChangeFlags,
  type FramePacketView,
} from "@galeon/render-core";
import type { RendererCache } from "@galeon/three";
import type * as THREE from "three";

export interface GaleonEntityRef {
  readonly key: string;
  readonly entityId: number;
  generation: number;
  parentId: number;
  objectType: number;
  visible: boolean;
  readonly transform: Float32Array;
  object: THREE.Object3D | undefined;
}

function makeEntityKey(entityId: number, generation: number): string {
  return `${entityId}:${generation}`;
}

export class GaleonEntityStore {
  private readonly byKey = new Map<string, GaleonEntityRef>();
  private readonly keyByEntityId = new Map<number, string>();
  private ordered: GaleonEntityRef[] = [];

  sync(packet: FramePacketView, cache: RendererCache): boolean {
    let structuralChanged = false;

    if (hasIncrementalChangeFlags(packet)) {
      for (let i = 0; i < packet.entity_count; i++) {
        const { entity, created } = this.upsertEntity(packet, i, cache);
        if (created) {
          this.ordered.push(entity);
          structuralChanged = true;
        }
      }
      return structuralChanged;
    }

    const activeKeys = new Set<string>();
    const nextOrder: GaleonEntityRef[] = [];
    for (let i = 0; i < packet.entity_count; i++) {
      const { entity, created } = this.upsertEntity(packet, i, cache);
      structuralChanged ||= created;
      activeKeys.add(entity.key);
      nextOrder.push(entity);
    }

    for (const key of Array.from(this.byKey.keys())) {
      if (!activeKeys.has(key)) {
        this.removeByKey(key);
        structuralChanged = true;
      }
    }

    structuralChanged ||= this.hasOrderChanged(nextOrder);
    this.ordered = nextOrder;
    return structuralChanged;
  }

  entities(): readonly GaleonEntityRef[] {
    return this.ordered;
  }

  get(entityId: number, generation: number): GaleonEntityRef | undefined {
    return this.byKey.get(makeEntityKey(entityId, generation));
  }

  clear(): void {
    this.byKey.clear();
    this.keyByEntityId.clear();
    this.ordered = [];
  }

  private upsertEntity(
    packet: FramePacketView,
    row: number,
    cache: RendererCache,
  ): { entity: GaleonEntityRef; created: boolean } {
    const entityId = packet.entity_ids[row]!;
    const generation = packet.entity_generations[row]!;
    const key = makeEntityKey(entityId, generation);

    const previousKey = this.keyByEntityId.get(entityId);
    if (previousKey !== undefined && previousKey !== key) {
      this.removeByKey(previousKey);
    }

    let entity = this.byKey.get(key);
    let created = false;
    if (entity === undefined) {
      entity = {
        key,
        entityId,
        generation,
        parentId: SCENE_ROOT,
        objectType: ObjectType.Mesh,
        visible: true,
        transform: new Float32Array(TRANSFORM_STRIDE),
        object: undefined,
      };
      this.byKey.set(key, entity);
      created = true;
    }

    this.keyByEntityId.set(entityId, key);
    entity.generation = generation;
    entity.parentId = packet.parent_ids[row] ?? SCENE_ROOT;
    entity.objectType = packet.object_types?.[row] ?? ObjectType.Mesh;
    entity.visible = packet.visibility[row] === 1;
    entity.object = cache.getObject(entityId, generation);

    const offset = row * TRANSFORM_STRIDE;
    entity.transform.set(packet.transforms.subarray(offset, offset + TRANSFORM_STRIDE));

    return { entity, created };
  }

  private removeByKey(key: string): void {
    const entity = this.byKey.get(key);
    if (entity === undefined) {
      return;
    }

    this.byKey.delete(key);
    if (this.keyByEntityId.get(entity.entityId) === key) {
      this.keyByEntityId.delete(entity.entityId);
    }

    const index = this.ordered.indexOf(entity);
    if (index >= 0) {
      this.ordered.splice(index, 1);
    }
  }

  private hasOrderChanged(nextOrder: readonly GaleonEntityRef[]): boolean {
    if (this.ordered.length !== nextOrder.length) {
      return true;
    }

    for (let i = 0; i < nextOrder.length; i++) {
      if (this.ordered[i] !== nextOrder[i]) {
        return true;
      }
    }

    return false;
  }
}
