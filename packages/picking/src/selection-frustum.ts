// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";

/**
 * Build a six-plane sub-frustum bounded by a screen-space rectangle.
 *
 * Conceptually the SelectionBox.js algorithm from three.js examples
 * (`examples/jsm/interactive/SelectionBox.js`): unproject the rect's four NDC
 * corners through the camera at near and far planes, then derive six planes
 * via `Plane.setFromCoplanarPoints`. The resulting `Frustum` can be tested
 * against world-space AABBs via `frustum.intersectsBox`.
 *
 * Plane normals are oriented inward by checking each one against the centroid
 * of the eight corner points (always interior to the sub-volume) and flipping
 * any that point the wrong way. This sidesteps the camera-handedness traps
 * that the textbook SelectionBox.js orderings hit when used outside their
 * tested perspective-looking-down-`-Z` camera setup.
 *
 * NDC inputs use the same convention as Three.js raycasting: x/y in [-1, 1]
 * with y flipped from screen-space (top is +1, bottom is -1).
 *
 * @param ndcStart First rect corner in NDC.
 * @param ndcEnd Opposite corner in NDC.
 * @param camera Camera used for unprojection. Must have an up-to-date world matrix.
 * @returns A `THREE.Frustum` with inward-pointing plane normals.
 */
export function frustumFromRect(
  ndcStart: { readonly x: number; readonly y: number },
  ndcEnd: { readonly x: number; readonly y: number },
  camera: THREE.Camera,
): THREE.Frustum {
  const minX = Math.min(ndcStart.x, ndcEnd.x);
  const maxX = Math.max(ndcStart.x, ndcEnd.x);
  const minY = Math.min(ndcStart.y, ndcEnd.y);
  const maxY = Math.max(ndcStart.y, ndcEnd.y);

  const nTL = unproject(minX, maxY, -1, camera);
  const nTR = unproject(maxX, maxY, -1, camera);
  const nBR = unproject(maxX, minY, -1, camera);
  const nBL = unproject(minX, minY, -1, camera);
  const fTL = unproject(minX, maxY, 1, camera);
  const fTR = unproject(maxX, maxY, 1, camera);
  const fBR = unproject(maxX, minY, 1, camera);
  const fBL = unproject(minX, minY, 1, camera);

  const centroid = new THREE.Vector3();
  for (const p of [nTL, nTR, nBR, nBL, fTL, fTR, fBR, fBL]) centroid.add(p);
  centroid.multiplyScalar(1 / 8);

  const frustum = new THREE.Frustum();
  setOrientedPlane(frustum.planes[0]!, nTL, fTL, fBL, centroid); // left
  setOrientedPlane(frustum.planes[1]!, nTR, fTR, fTL, centroid); // top
  setOrientedPlane(frustum.planes[2]!, nBR, fBR, fTR, centroid); // right
  setOrientedPlane(frustum.planes[3]!, nBL, fBL, fBR, centroid); // bottom
  setOrientedPlane(frustum.planes[4]!, nTL, nTR, nBR, centroid); // near
  setOrientedPlane(frustum.planes[5]!, fTL, fTR, fBR, centroid); // far
  return frustum;
}

function unproject(
  x: number,
  y: number,
  z: number,
  camera: THREE.Camera,
): THREE.Vector3 {
  return new THREE.Vector3(x, y, z).unproject(camera);
}

function setOrientedPlane(
  plane: THREE.Plane,
  a: THREE.Vector3,
  b: THREE.Vector3,
  c: THREE.Vector3,
  interior: THREE.Vector3,
): void {
  plane.setFromCoplanarPoints(a, b, c);
  if (plane.distanceToPoint(interior) < 0) {
    plane.normal.negate();
    plane.constant = -plane.constant;
  }
}
