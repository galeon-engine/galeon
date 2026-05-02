// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  attachSelectionRings,
  type SelectionRingInstance,
  type SelectionRingObjectResolver,
} from "../src/selection-rings.js";

class FakeResolver implements SelectionRingObjectResolver {
  readonly objects = new Map<string, THREE.Object3D>();
  readonly instances = new Map<string, SelectionRingInstance>();

  set(entityId: number, generation: number, object: THREE.Object3D): void {
    this.objects.set(`${entityId}:${generation}`, object);
  }

  setInstance(entityId: number, generation: number, instance: SelectionRingInstance): void {
    this.instances.set(`${entityId}:${generation}`, instance);
  }

  getObject(entityId: number, generation: number): THREE.Object3D | undefined {
    return this.objects.get(`${entityId}:${generation}`);
  }

  getInstance(entityId: number, generation: number): SelectionRingInstance | undefined {
    return this.instances.get(`${entityId}:${generation}`);
  }
}

describe("attachSelectionRings", () => {
  test("draws one ring for each resolved selected entity", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(2, 1, 4), new THREE.MeshBasicMaterial());
    mesh.position.set(3, 0, 5);
    scene.add(mesh);
    resolver.set(1, 0, mesh);

    const rings = attachSelectionRings(scene, resolver);
    rings.update([{ entityId: 1, generation: 0 }]);

    expect(scene.children).toContain(rings.group);
    expect(rings.group.children).toHaveLength(1);
    const ring = rings.group.children[0]!;
    const position = new THREE.Vector3();
    const scale = new THREE.Vector3();
    const quaternion = new THREE.Quaternion();
    ring.matrix.decompose(position, quaternion, scale);

    expect(position.x).toBeCloseTo(3);
    expect(position.y).toBeCloseTo(-0.47);
    expect(position.z).toBeCloseTo(5);
    expect(scale.x).toBeCloseTo(1.18);
    expect(scale.z).toBeCloseTo(2.36);
  });

  test("removes rings for deselected or unresolved entities", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    resolver.set(1, 0, new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial()));
    const rings = attachSelectionRings(scene, resolver);

    rings.update([{ entityId: 1, generation: 0 }]);
    expect(rings.group.children).toHaveLength(1);

    rings.update([{ entityId: 2, generation: 0 }]);
    expect(rings.group.children).toHaveLength(0);
  });

  test("draws rings for selected instanced mesh slots", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    const mesh = new THREE.InstancedMesh(
      new THREE.BoxGeometry(2, 1, 4),
      new THREE.MeshBasicMaterial(),
      1,
    );
    const matrix = new THREE.Matrix4().compose(
      new THREE.Vector3(3, 0, 5),
      new THREE.Quaternion(),
      new THREE.Vector3(2, 1, 0.5),
    );
    mesh.setMatrixAt(0, matrix);
    mesh.count = 1;
    scene.add(mesh);
    resolver.setInstance(1, 0, { mesh, instanceId: 0 });

    const rings = attachSelectionRings(scene, resolver);
    rings.update([{ entityId: 1, generation: 0 }]);

    expect(rings.group.children).toHaveLength(1);
    const ring = rings.group.children[0]!;
    const position = new THREE.Vector3();
    const scale = new THREE.Vector3();
    ring.matrix.decompose(position, new THREE.Quaternion(), scale);

    expect(position.x).toBeCloseTo(3);
    expect(position.y).toBeCloseTo(-0.47);
    expect(position.z).toBeCloseTo(5);
    expect(scale.x).toBeCloseTo(2.36);
    expect(scale.z).toBeCloseTo(1.18);
  });

  test("skips hidden object chains", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    const parent = new THREE.Group();
    parent.visible = false;
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    parent.add(mesh);
    scene.add(parent);
    resolver.set(1, 0, mesh);
    const rings = attachSelectionRings(scene, resolver);

    rings.update([{ entityId: 1, generation: 0 }]);

    expect(rings.group.children).toHaveLength(0);
  });

  test("dispose removes the overlay group from the scene", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    const rings = attachSelectionRings(scene, resolver);

    expect(scene.children).toContain(rings.group);
    rings.dispose();

    expect(scene.children).not.toContain(rings.group);
    expect(rings.group.children).toHaveLength(0);
  });

  test("empty object bounds fall back to world position and min radius", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    const group = new THREE.Group();
    group.position.set(4, 5, 6);
    scene.add(group);
    resolver.set(1, 0, group);
    const rings = attachSelectionRings(scene, resolver, { minRadius: 0.75 });

    rings.update([{ entityId: 1, generation: 0 }]);

    const ring = rings.group.children[0]!;
    const position = new THREE.Vector3();
    const scale = new THREE.Vector3();
    ring.matrix.decompose(position, new THREE.Quaternion(), scale);
    expect(position.x).toBeCloseTo(4);
    expect(position.y).toBeCloseTo(5);
    expect(position.z).toBeCloseTo(6);
    expect(scale.x).toBeCloseTo(0.75);
    expect(scale.z).toBeCloseTo(0.75);
  });

  test("does not dispose caller-owned geometry or material", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    const geometry = new THREE.BufferGeometry();
    const material = new THREE.LineBasicMaterial();
    let geometryDisposed = false;
    let materialDisposed = false;
    geometry.addEventListener("dispose", () => {
      geometryDisposed = true;
    });
    material.addEventListener("dispose", () => {
      materialDisposed = true;
    });

    const rings = attachSelectionRings(scene, resolver, { geometry, material });
    rings.dispose();

    expect(geometryDisposed).toBe(false);
    expect(materialDisposed).toBe(false);
  });

  test("updates selected object matrices without forcing the whole scene", () => {
    const scene = new THREE.Scene();
    const resolver = new FakeResolver();
    let sceneUpdates = 0;
    const originalSceneUpdate = scene.updateMatrixWorld.bind(scene);
    scene.updateMatrixWorld = (force?: boolean) => {
      sceneUpdates += 1;
      originalSceneUpdate(force);
    };
    const object = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial());
    let selectedOnlyUpdates = 0;
    let fullHierarchyUpdates = 0;
    const originalObjectUpdate = object.updateWorldMatrix.bind(object);
    object.updateWorldMatrix = (updateParents: boolean, updateChildren: boolean) => {
      if (updateParents && !updateChildren) selectedOnlyUpdates += 1;
      if (updateParents && updateChildren) fullHierarchyUpdates += 1;
      originalObjectUpdate(updateParents, updateChildren);
    };
    scene.add(object);
    resolver.set(1, 0, object);

    const rings = attachSelectionRings(scene, resolver);
    rings.update([{ entityId: 1, generation: 0 }]);

    expect(sceneUpdates).toBe(0);
    expect(selectedOnlyUpdates).toBe(1);
    expect(fullHierarchyUpdates).toBe(0);
  });
});
