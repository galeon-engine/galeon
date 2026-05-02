// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import type { PickingEntityRef } from "./picking.js";

export interface SelectionRingObjectResolver {
  getObject(entityId: number, generation: number): THREE.Object3D | undefined;
}

export interface SelectionRingsOptions {
  readonly color?: THREE.ColorRepresentation;
  readonly opacity?: number;
  readonly radiusScale?: number;
  readonly minRadius?: number;
  readonly yOffset?: number;
  readonly segments?: number;
  readonly renderOrder?: number;
  readonly geometry?: THREE.BufferGeometry;
  readonly material?: THREE.LineBasicMaterial;
}

export interface SelectionRingsController {
  readonly group: THREE.Group;
  update(selection: readonly PickingEntityRef[]): void;
  dispose(): void;
}

const DEFAULT_COLOR = 0x50a0ff;
const DEFAULT_OPACITY = 0.95;
const DEFAULT_RADIUS_SCALE = 1.18;
const DEFAULT_MIN_RADIUS = 0.25;
const DEFAULT_Y_OFFSET = 0.03;
const DEFAULT_SEGMENTS = 64;
const DEFAULT_RENDER_ORDER = 10_000;

const _box = new THREE.Box3();
const _center = new THREE.Vector3();
const _size = new THREE.Vector3();
const _position = new THREE.Vector3();
const _scale = new THREE.Vector3();
const _quaternion = new THREE.Quaternion();

/**
 * Draw world-space selection rings for the current selected entity refs.
 *
 * Rings are simple Three.js `LineLoop`s in the XZ plane. The helper owns only
 * the overlay group it adds to `scene`; it reads selected entity objects from a
 * resolver such as `RendererCache` and never mutates engine selection state.
 */
export function attachSelectionRings(
  scene: THREE.Scene,
  target: SelectionRingObjectResolver,
  options: SelectionRingsOptions = {},
): SelectionRingsController {
  const group = new THREE.Group();
  group.name = "GaleonSelectionRings";
  group.matrixAutoUpdate = false;
  scene.add(group);

  const ownsGeometry = options.geometry === undefined;
  const ownsMaterial = options.material === undefined;
  const geometry = options.geometry ?? createUnitRingGeometry(options.segments ?? DEFAULT_SEGMENTS);
  const material = options.material ?? new THREE.LineBasicMaterial({
    color: options.color ?? DEFAULT_COLOR,
    depthTest: false,
    depthWrite: false,
    opacity: options.opacity ?? DEFAULT_OPACITY,
    transparent: true,
    toneMapped: false,
  });
  const rings = new Map<string, THREE.LineLoop>();

  const controller: SelectionRingsController = {
    group,
    update(selection: readonly PickingEntityRef[]): void {
      const active = new Set<string>();
      scene.updateMatrixWorld(true);
      for (const entity of selection) {
        const object = target.getObject(entity.entityId, entity.generation);
        if (object === undefined || !visibleObjectChain(object)) {
          continue;
        }
        const key = selectionKey(entity);
        active.add(key);
        let ring = rings.get(key);
        if (ring === undefined) {
          ring = new THREE.LineLoop(geometry, material);
          ring.matrixAutoUpdate = false;
          ring.renderOrder = options.renderOrder ?? DEFAULT_RENDER_ORDER;
          rings.set(key, ring);
          group.add(ring);
        }
        updateRingMatrix(ring, object, options);
      }

      for (const [key, ring] of rings) {
        if (active.has(key)) continue;
        group.remove(ring);
        rings.delete(key);
      }
    },
    dispose(): void {
      for (const ring of rings.values()) {
        group.remove(ring);
      }
      rings.clear();
      group.parent?.remove(group);
      if (ownsGeometry) geometry.dispose();
      if (ownsMaterial) material.dispose();
    },
  };

  return controller;
}

function selectionKey(entity: PickingEntityRef): string {
  return `${entity.entityId}:${entity.generation}`;
}

function updateRingMatrix(
  ring: THREE.LineLoop,
  object: THREE.Object3D,
  options: SelectionRingsOptions,
): void {
  _box.setFromObject(object);
  const minRadius = options.minRadius ?? DEFAULT_MIN_RADIUS;
  if (_box.isEmpty()) {
    object.getWorldPosition(_position);
    _scale.set(minRadius, 1, minRadius);
  } else {
    _box.getCenter(_center);
    _box.getSize(_size);
    _position.set(
      _center.x,
      _box.min.y + (options.yOffset ?? DEFAULT_Y_OFFSET),
      _center.z,
    );
    const radiusScale = options.radiusScale ?? DEFAULT_RADIUS_SCALE;
    _scale.set(
      Math.max((_size.x * radiusScale) / 2, minRadius),
      1,
      Math.max((_size.z * radiusScale) / 2, minRadius),
    );
  }
  ring.matrix.compose(_position, _quaternion, _scale);
  ring.matrixWorldNeedsUpdate = true;
}

function createUnitRingGeometry(segments: number): THREE.BufferGeometry {
  const points: THREE.Vector3[] = [];
  const safeSegments = Math.max(8, Math.floor(segments));
  for (let i = 0; i < safeSegments; i++) {
    const t = (i / safeSegments) * Math.PI * 2;
    points.push(new THREE.Vector3(Math.cos(t), 0, Math.sin(t)));
  }
  return new THREE.BufferGeometry().setFromPoints(points);
}

function visibleObjectChain(object: THREE.Object3D): boolean {
  let current: THREE.Object3D | null = object;
  while (current !== null) {
    if (!current.visible) return false;
    current = current.parent;
  }
  return true;
}
