// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

import {
  spawnBurst,
  updateParticles,
  type BillboardParticle,
} from "./simulation.js";

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setSize(window.innerWidth, window.innerHeight);
renderer.setPixelRatio(window.devicePixelRatio);
const appEl = document.getElementById("app");
if (appEl === null) {
  throw new Error("missing #app mount element");
}
appEl.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x101820);

const camera = new THREE.PerspectiveCamera(
  60,
  window.innerWidth / window.innerHeight,
  0.1,
  200,
);
camera.position.set(8, 7, 12);

const controls = new OrbitControls(camera, renderer.domElement);
controls.target.set(0, 1, 0);
controls.enableDamping = true;
controls.update();

scene.add(new THREE.AmbientLight(0xffffff, 0.4));
const sun = new THREE.DirectionalLight(0xffffff, 0.85);
sun.position.set(5, 10, 5);
scene.add(sun);

const groundGeo = new THREE.PlaneGeometry(40, 40);
const groundMat = new THREE.MeshStandardMaterial({
  color: 0x1a2030,
  roughness: 1,
});
const ground = new THREE.Mesh(groundGeo, groundMat);
ground.rotation.x = -Math.PI / 2;
scene.add(ground);

scene.add(new THREE.GridHelper(40, 40, 0x223040, 0x223040));

// Shared unit-quad geometry for every billboard. The vertex shader places
// it in view space at the mesh's world origin and offsets corners by uSize.
const billboardGeometry = new THREE.PlaneGeometry(1, 1);
const particles: BillboardParticle[] = [];

const raycaster = new THREE.Raycaster();
const ndc = new THREE.Vector2();
const countEl = document.getElementById("count");

// A confirmed click is pointerdown + pointerup on the same primary button
// without a drag in between. Binding spawn to pointerdown would also fire
// at the start of every OrbitControls left-drag gesture.
const CLICK_DRAG_THRESHOLD_PX = 5;
let pointerDownAt: { x: number; y: number; pointerId: number } | null = null;

renderer.domElement.addEventListener("pointerdown", (event) => {
  if (event.button !== 0) {
    return;
  }
  pointerDownAt = {
    x: event.clientX,
    y: event.clientY,
    pointerId: event.pointerId,
  };
});

renderer.domElement.addEventListener("pointerup", (event) => {
  const start = pointerDownAt;
  pointerDownAt = null;
  if (start === null || event.pointerId !== start.pointerId) {
    return;
  }
  if (event.button !== 0) {
    return;
  }
  const dx = event.clientX - start.x;
  const dy = event.clientY - start.y;
  if (dx * dx + dy * dy > CLICK_DRAG_THRESHOLD_PX * CLICK_DRAG_THRESHOLD_PX) {
    return;
  }
  const rect = renderer.domElement.getBoundingClientRect();
  ndc.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
  ndc.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
  raycaster.setFromCamera(ndc, camera);
  const hits = raycaster.intersectObject(ground, false);
  if (hits.length === 0) {
    return;
  }
  const hit = hits[0]!;
  const fresh = spawnBurst({
    origin: hit.point,
    geometry: billboardGeometry,
    scene,
  });
  particles.push(...fresh);
});

renderer.domElement.addEventListener("pointercancel", () => {
  pointerDownAt = null;
});

window.addEventListener("resize", () => {
  renderer.setSize(window.innerWidth, window.innerHeight);
  camera.aspect = window.innerWidth / window.innerHeight;
  camera.updateProjectionMatrix();
});

let lastTs = performance.now() / 1000;
function tick(): void {
  const now = performance.now() / 1000;
  // Clamp delta to prevent huge spikes on tab refocus from killing the
  // simulation in a single frame.
  const dt = Math.min(now - lastTs, 0.05);
  lastTs = now;

  updateParticles(particles, dt, scene);
  if (countEl !== null) {
    countEl.textContent = String(particles.length);
  }

  controls.update();
  renderer.render(scene, camera);
  requestAnimationFrame(tick);
}
requestAnimationFrame(tick);
