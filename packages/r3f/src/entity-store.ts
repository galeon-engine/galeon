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
    if (hasIncrementalChangeFlags(packet)) {
      const results = GaleonEntityStore.rowIndexes(packet).map((row) =>
        this.upsertEntity(packet, row, cache),
      );
      const changedEntities = results.filter(
        ({ created, objectChanged }) => created || objectChanged,
      );
      const createdEntities = results
        .filter(({ created }) => created)
        .map(({ entity }) => entity);
      if (createdEntities.length > 0) {
        this.ordered = [...this.ordered, ...createdEntities];
      }
      return changedEntities.length > 0;
    }

    const activeKeys = new Set<string>();
    const rows = GaleonEntityStore.rowIndexes(packet).map((row) => {
      const result = this.upsertEntity(packet, row, cache);
      const { entity } = result;
      activeKeys.add(entity.key);
      return result;
    });
    const nextOrder = rows.map(({ entity }) => entity);
    const structuralChanged =
      rows.some(({ created, objectChanged }) => created || objectChanged)
      || this.hasOrderChanged(nextOrder);

    const removedCount = Array.from(this.byKey.keys())
      .filter((key) => !activeKeys.has(key))
      .map((key) => this.removeByKey(key))
      .filter((entity) => entity !== undefined).length;

    this.ordered = nextOrder;
    return structuralChanged || removedCount > 0;
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
  ): { entity: GaleonEntityRef; created: boolean; objectChanged: boolean } {
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

    const previousObject = entity.object;
    this.keyByEntityId.set(entityId, key);
    entity.generation = generation;
    entity.parentId = packet.parent_ids[row] ?? SCENE_ROOT;
    entity.objectType = packet.object_types?.[row] ?? ObjectType.Mesh;
    entity.visible = packet.visibility[row] === 1;
    entity.object = cache.getObject(entityId, generation);
    const objectChanged = previousObject !== entity.object;

    const offset = row * TRANSFORM_STRIDE;
    entity.transform.set(packet.transforms.subarray(offset, offset + TRANSFORM_STRIDE));

    return { entity, created, objectChanged };
  }

  private removeByKey(key: string): GaleonEntityRef | undefined {
    const entity = this.byKey.get(key);
    if (entity === undefined) {
      return undefined;
    }

    this.byKey.delete(key);
    if (this.keyByEntityId.get(entity.entityId) === key) {
      this.keyByEntityId.delete(entity.entityId);
    }

    this.ordered = this.ordered.filter((existing) => existing !== entity);
    return entity;
  }

  private static rowIndexes(packet: FramePacketView): number[] {
    return Array.from({ length: packet.entity_count }, (_, index) => index);
  }

  private hasOrderChanged(nextOrder: readonly GaleonEntityRef[]): boolean {
    if (this.ordered.length !== nextOrder.length) {
      return true;
    }

    return nextOrder.some((entity, index) => this.ordered[index] !== entity);
  }
}
