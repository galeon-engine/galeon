// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";

export interface MarqueeRect {
  readonly start: THREE.Vector2 | { readonly x: number; readonly y: number };
  readonly end: THREE.Vector2 | { readonly x: number; readonly y: number };
}

export interface MarqueeRendererOptions {
  readonly color?: THREE.ColorRepresentation;
  readonly opacity?: number;
  readonly renderOrder?: number;
  readonly zOffset?: number;
  readonly material?: THREE.LineBasicMaterial;
}

export interface MarqueeRendererController {
  readonly line: THREE.LineLoop;
  update(rect: MarqueeRect | null): void;
  dispose(): void;
}

const DEFAULT_COLOR = 0x50a0ff;
const DEFAULT_OPACITY = 0.95;
const DEFAULT_RENDER_ORDER = 10_001;
const DEFAULT_Z_OFFSET = -0.02;

/**
 * Render a drag rectangle as camera-attached Three.js geometry.
 *
 * The rectangle is supplied in Normalised Device Coordinates so callers can
 * reuse the same pointer-to-NDC math that drives `attachPicking`. It is
 * visual-only: the helper never emits picking events or mutates selection.
 */
export function attachMarqueeRenderer(
  camera: THREE.Camera,
  options: MarqueeRendererOptions = {},
): MarqueeRendererController {
  const ownsMaterial = options.material === undefined;
  const geometry = createRectGeometry();
  const material = options.material ?? new THREE.LineBasicMaterial({
    color: options.color ?? DEFAULT_COLOR,
    depthTest: false,
    depthWrite: false,
    opacity: options.opacity ?? DEFAULT_OPACITY,
    transparent: true,
    toneMapped: false,
  });
  const line = new THREE.LineLoop(geometry, material);
  line.name = "GaleonMarqueeRenderer";
  line.frustumCulled = false;
  line.matrixAutoUpdate = false;
  line.renderOrder = options.renderOrder ?? DEFAULT_RENDER_ORDER;
  line.visible = false;
  camera.add(line);

  return {
    line,
    update(rect: MarqueeRect | null): void {
      if (rect == null) {
        line.visible = false;
        return;
      }
      writeRectGeometry(geometry, rect, options.zOffset ?? DEFAULT_Z_OFFSET);
      line.visible = true;
    },
    dispose(): void {
      line.parent?.remove(line);
      geometry.dispose();
      if (ownsMaterial) material.dispose();
    },
  };
}

function createRectGeometry(): THREE.BufferGeometry {
  return new THREE.BufferGeometry().setAttribute(
    "position",
    new THREE.BufferAttribute(new Float32Array(12), 3),
  );
}

function writeRectGeometry(
  geometry: THREE.BufferGeometry,
  rect: MarqueeRect,
  z: number,
): void {
  const x0 = Math.min(rect.start.x, rect.end.x);
  const x1 = Math.max(rect.start.x, rect.end.x);
  const y0 = Math.min(rect.start.y, rect.end.y);
  const y1 = Math.max(rect.start.y, rect.end.y);
  const attr = geometry.getAttribute("position") as THREE.BufferAttribute;
  attr.setXYZ(0, x0, y0, z);
  attr.setXYZ(1, x1, y0, z);
  attr.setXYZ(2, x1, y1, z);
  attr.setXYZ(3, x0, y1, z);
  attr.needsUpdate = true;
  geometry.computeBoundingSphere();
}
