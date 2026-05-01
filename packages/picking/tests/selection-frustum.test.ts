// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { frustumFromRect } from "../src/selection-frustum.js";

function makeOrthoCamera(): THREE.OrthographicCamera {
  const camera = new THREE.OrthographicCamera(-2, 2, 2, -2, 0.1, 100);
  camera.position.set(0, 0, 5);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  return camera;
}

function makePerspectiveCamera(): THREE.PerspectiveCamera {
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
  camera.position.set(0, 0, 5);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  return camera;
}

function box(min: [number, number, number], max: [number, number, number]): THREE.Box3 {
  return new THREE.Box3(new THREE.Vector3(...min), new THREE.Vector3(...max));
}

describe("frustumFromRect", () => {
  test("full-screen rect contains every box visible to an ortho camera", () => {
    const frustum = frustumFromRect({ x: -1, y: -1 }, { x: 1, y: 1 }, makeOrthoCamera());
    expect(frustum.intersectsBox(box([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]))).toBe(true);
    expect(frustum.intersectsBox(box([1.5, 1.5, -0.5], [1.6, 1.6, 0.5]))).toBe(true);
    expect(frustum.intersectsBox(box([3, 3, 0], [3.1, 3.1, 0.1]))).toBe(false);
  });

  test("upper-right quadrant excludes boxes in lower-left", () => {
    const frustum = frustumFromRect({ x: 0, y: 0 }, { x: 1, y: 1 }, makeOrthoCamera());
    expect(frustum.intersectsBox(box([0.5, 0.5, -0.5], [1, 1, 0.5]))).toBe(true);
    expect(frustum.intersectsBox(box([-1.5, -1.5, -0.5], [-0.5, -0.5, 0.5]))).toBe(false);
  });

  test("works with a perspective camera", () => {
    const frustum = frustumFromRect({ x: -1, y: -1 }, { x: 1, y: 1 }, makePerspectiveCamera());
    // Origin is in view of the perspective camera.
    expect(frustum.intersectsBox(box([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]))).toBe(true);
    // Behind the camera should be excluded.
    expect(frustum.intersectsBox(box([0, 0, 50], [0.1, 0.1, 50.1]))).toBe(false);
  });

  test("rect endpoints can be supplied in any order", () => {
    const a = frustumFromRect({ x: -0.5, y: -0.5 }, { x: 0.5, y: 0.5 }, makeOrthoCamera());
    const b = frustumFromRect({ x: 0.5, y: 0.5 }, { x: -0.5, y: -0.5 }, makeOrthoCamera());
    const test = box([-0.2, -0.2, -0.5], [0.2, 0.2, 0.5]);
    expect(a.intersectsBox(test)).toBe(b.intersectsBox(test));
  });
});
