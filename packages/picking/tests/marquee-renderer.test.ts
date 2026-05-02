// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { attachMarqueeRenderer } from "../src/marquee-renderer.js";

describe("attachMarqueeRenderer", () => {
  test("attaches a hidden line loop to the camera", () => {
    const camera = new THREE.PerspectiveCamera();
    const marquee = attachMarqueeRenderer(camera);

    expect(camera.children).toContain(marquee.line);
    expect(marquee.line.visible).toBe(false);
  });

  test("updates rectangle geometry from NDC endpoints", () => {
    const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 100);
    const marquee = attachMarqueeRenderer(camera);

    marquee.update({
      start: { x: 0.5, y: -0.25 },
      end: { x: -0.5, y: 0.75 },
    });

    const positions = marquee.line.geometry.getAttribute("position");
    expect(marquee.line.visible).toBe(true);
    expect([positions.getX(0), positions.getY(0)]).toEqual([-0.5, -0.25]);
    expect([positions.getX(1), positions.getY(1)]).toEqual([0.5, -0.25]);
    expect([positions.getX(2), positions.getY(2)]).toEqual([0.5, 0.75]);
    expect([positions.getX(3), positions.getY(3)]).toEqual([-0.5, 0.75]);
  });

  test("places perspective marquee beyond the near plane by default", () => {
    const camera = new THREE.PerspectiveCamera(50, 1, 0.1, 100);
    const marquee = attachMarqueeRenderer(camera);

    marquee.update({
      start: { x: -1, y: -1 },
      end: { x: 1, y: 1 },
    });

    const positions = marquee.line.geometry.getAttribute("position");
    expect(positions.getZ(0)).toBeLessThan(-camera.near);
  });

  test("keeps default perspective marquee inside a tight far plane", () => {
    const camera = new THREE.PerspectiveCamera(50, 1, 1, 1.5);
    const marquee = attachMarqueeRenderer(camera);

    marquee.update({
      start: { x: -1, y: -1 },
      end: { x: 1, y: 1 },
    });

    const positions = marquee.line.geometry.getAttribute("position");
    const depth = Math.abs(positions.getZ(0));
    expect(depth).toBeGreaterThan(camera.near);
    expect(depth).toBeLessThan(camera.far);
  });

  test("projects perspective marquee through camera zoom", () => {
    const camera = new THREE.PerspectiveCamera(90, 1, 0.1, 100);
    camera.zoom = 2;
    const marquee = attachMarqueeRenderer(camera, { zOffset: -2 });

    marquee.update({
      start: { x: -1, y: -1 },
      end: { x: 1, y: 1 },
    });

    const positions = marquee.line.geometry.getAttribute("position");
    expect(positions.getX(0)).toBeCloseTo(-1);
    expect(positions.getY(0)).toBeCloseTo(-1);
    expect(positions.getX(2)).toBeCloseTo(1);
    expect(positions.getY(2)).toBeCloseTo(1);
  });

  test("keeps off-center orthographic marquee centered while zoomed", () => {
    const camera = new THREE.OrthographicCamera(2, 6, 8, 4, 0.1, 100);
    camera.zoom = 2;
    const marquee = attachMarqueeRenderer(camera);

    marquee.update({
      start: { x: -1, y: -1 },
      end: { x: 1, y: 1 },
    });

    const positions = marquee.line.geometry.getAttribute("position");
    expect(positions.getX(0)).toBeCloseTo(3);
    expect(positions.getY(0)).toBeCloseTo(5);
    expect(positions.getX(2)).toBeCloseTo(5);
    expect(positions.getY(2)).toBeCloseTo(7);
  });

  test("hides on null update and disposes the camera child", () => {
    const camera = new THREE.PerspectiveCamera();
    const marquee = attachMarqueeRenderer(camera);

    marquee.update({ start: { x: -1, y: -1 }, end: { x: 1, y: 1 } });
    marquee.update(null);
    expect(marquee.line.visible).toBe(false);

    marquee.dispose();
    expect(camera.children).not.toContain(marquee.line);
  });

  test("does not dispose caller-owned material", () => {
    const camera = new THREE.PerspectiveCamera();
    const material = new THREE.LineBasicMaterial();
    let disposed = false;
    material.addEventListener("dispose", () => {
      disposed = true;
    });

    attachMarqueeRenderer(camera, { material }).dispose();

    expect(disposed).toBe(false);
  });
});
