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
  /** Camera-local negative Z plane. Defaults to just beyond `camera.near`. */
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
      writeRectGeometry(geometry, camera, rect, resolveZOffset(camera, options.zOffset));
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
  camera: THREE.Camera,
  rect: MarqueeRect,
  z: number,
): void {
  const x0 = Math.min(rect.start.x, rect.end.x);
  const x1 = Math.max(rect.start.x, rect.end.x);
  const y0 = Math.min(rect.start.y, rect.end.y);
  const y1 = Math.max(rect.start.y, rect.end.y);
  const p0 = ndcToCameraLocal(camera, x0, y0, z);
  const p1 = ndcToCameraLocal(camera, x1, y0, z);
  const p2 = ndcToCameraLocal(camera, x1, y1, z);
  const p3 = ndcToCameraLocal(camera, x0, y1, z);
  const attr = geometry.getAttribute("position") as THREE.BufferAttribute;
  attr.setXYZ(0, p0.x, p0.y, p0.z);
  attr.setXYZ(1, p1.x, p1.y, p1.z);
  attr.setXYZ(2, p2.x, p2.y, p2.z);
  attr.setXYZ(3, p3.x, p3.y, p3.z);
  attr.needsUpdate = true;
  geometry.computeBoundingSphere();
}

function resolveZOffset(camera: THREE.Camera, zOffset: number | undefined): number {
  if (zOffset !== undefined) return zOffset;
  const near = "near" in camera && typeof camera.near === "number" ? camera.near : 0.1;
  return -Math.max(near * 2, near + 0.01);
}

function ndcToCameraLocal(
  camera: THREE.Camera,
  x: number,
  y: number,
  z: number,
): THREE.Vector3 {
  if (camera instanceof THREE.PerspectiveCamera) {
    const depth = Math.abs(z);
    const halfHeight = Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2) * depth / camera.zoom;
    const halfWidth = halfHeight * camera.aspect;
    return new THREE.Vector3(x * halfWidth, y * halfHeight, z);
  }
  if (camera instanceof THREE.OrthographicCamera) {
    const halfWidth = (camera.right - camera.left) / (2 * camera.zoom);
    const halfHeight = (camera.top - camera.bottom) / (2 * camera.zoom);
    const centerX = (camera.left + camera.right) / 2;
    const centerY = (camera.top + camera.bottom) / 2;
    return new THREE.Vector3(centerX + x * halfWidth, centerY + y * halfHeight, z);
  }
  return new THREE.Vector3(x, y, z);
}
