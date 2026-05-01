// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/**
 * Galeon — instanced cubes benchmark.
 *
 * 5000 cubes drift through a sine field. The renderer can be exercised in
 * two modes via the URL query parameter:
 *
 *   ?mode=instanced   — every cube tagged with InstanceOf, the shared
 *                        THREE.InstancedMesh path. (default)
 *   ?mode=standalone  — every cube renders through the per-entity Object3D
 *                        path, the same as before issue #215.
 *
 * The simulation side is intentionally minimal: each frame we synthesize a
 * `FramePacketView` directly without going through wasm-bindgen. This keeps
 * the benchmark focused on `RendererCache` + `InstancedMeshManager` and
 * isolates renderer cost from any extraction overhead.
 */

import * as THREE from "three";
import {
  CHANGED_TRANSFORM,
  INSTANCE_GROUP_NONE,
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
} from "@galeon/render-core";
import { RendererCache } from "@galeon/three";

const ENTITY_COUNT = 5000;
const MESH_HANDLE_ID = 1;
const MATERIAL_HANDLE_ID = 1;

type Mode = "instanced" | "standalone";

function readMode(): Mode {
  const params = new URLSearchParams(globalThis.location?.search ?? "");
  const raw = params.get("mode");
  return raw === "standalone" ? "standalone" : "instanced";
}

function setupScene(): {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
} {
  const renderer = new THREE.WebGLRenderer({ antialias: false, powerPreference: "high-performance" });
  renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio ?? 1, 2));
  renderer.setSize(globalThis.innerWidth, globalThis.innerHeight);
  document.body.appendChild(renderer.domElement);

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0b0e13);
  scene.add(new THREE.AmbientLight(0xffffff, 0.4));
  const dir = new THREE.DirectionalLight(0xffffff, 1.0);
  dir.position.set(40, 80, 60);
  scene.add(dir);

  const camera = new THREE.PerspectiveCamera(
    60,
    globalThis.innerWidth / globalThis.innerHeight,
    0.1,
    2000,
  );
  camera.position.set(0, 60, 140);
  camera.lookAt(0, 0, 0);

  globalThis.addEventListener("resize", () => {
    camera.aspect = globalThis.innerWidth / globalThis.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(globalThis.innerWidth, globalThis.innerHeight);
  });

  return { renderer, scene, camera };
}

/**
 * Pre-allocate flat typed arrays once and mutate them every frame. This
 * mirrors what the WASM bridge would deliver — the renderer never sees
 * fresh allocations on the hot path.
 */
function allocPacketBuffers(count: number, mode: Mode): {
  entityIds: Uint32Array;
  generations: Uint32Array;
  transforms: Float32Array;
  visibility: Uint8Array;
  meshHandles: Uint32Array;
  materialHandles: Uint32Array;
  parentIds: Uint32Array;
  objectTypes: Uint8Array;
  instanceGroups: Uint32Array;
  tints: Float32Array;
} {
  const entityIds = new Uint32Array(count);
  const generations = new Uint32Array(count);
  const transforms = new Float32Array(count * TRANSFORM_STRIDE);
  const visibility = new Uint8Array(count).fill(1);
  const meshHandles = new Uint32Array(count).fill(MESH_HANDLE_ID);
  const materialHandles = new Uint32Array(count).fill(MATERIAL_HANDLE_ID);
  const parentIds = new Uint32Array(count).fill(SCENE_ROOT);
  const objectTypes = new Uint8Array(count); // 0 = Mesh
  const instanceGroups = new Uint32Array(count);
  if (mode === "instanced") {
    instanceGroups.fill(MESH_HANDLE_ID);
  } else {
    instanceGroups.fill(INSTANCE_GROUP_NONE);
  }
  const tints = new Float32Array(count * 3);

  for (let i = 0; i < count; i++) {
    entityIds[i] = i + 1;
    // Identity rotation; scale (1,1,1); position written per frame.
    transforms[i * TRANSFORM_STRIDE + 6] = 1;
    transforms[i * TRANSFORM_STRIDE + 7] = 1;
    transforms[i * TRANSFORM_STRIDE + 8] = 1;
    transforms[i * TRANSFORM_STRIDE + 9] = 1;
    // Bands of color along the grid so instanceColor variation is visible.
    const band = i % 3;
    tints[i * 3 + 0] = band === 0 ? 1 : 0.2;
    tints[i * 3 + 1] = band === 1 ? 1 : 0.2;
    tints[i * 3 + 2] = band === 2 ? 1 : 0.2;
  }

  return {
    entityIds,
    generations,
    transforms,
    visibility,
    meshHandles,
    materialHandles,
    parentIds,
    objectTypes,
    instanceGroups,
    tints,
  };
}

function buildPacket(
  buffers: ReturnType<typeof allocPacketBuffers>,
  count: number,
): FramePacketView {
  return {
    contract_version: RENDER_CONTRACT_VERSION,
    entity_count: count,
    entity_ids: buffers.entityIds,
    entity_generations: buffers.generations,
    transforms: buffers.transforms,
    visibility: buffers.visibility,
    mesh_handles: buffers.meshHandles,
    material_handles: buffers.materialHandles,
    parent_ids: buffers.parentIds,
    object_types: buffers.objectTypes,
    instance_groups: buffers.instanceGroups,
    tints: buffers.tints,
    custom_channel_count: 0,
    custom_channel_name_at: () => "",
    custom_channel_stride: () => 1,
    custom_channel_data: () => new Float32Array(0),
    event_count: 0,
    event_kinds: new Uint32Array(0),
    event_entities: new Uint32Array(0),
    event_positions: new Float32Array(0),
    event_intensities: new Float32Array(0),
    event_data: new Float32Array(0),
    // Mark every row as transform-changed so RendererCache writes the
    // matrix every frame in both modes — keeps the comparison fair.
    change_flags: makeAllChangedFlags(count),
  };
}

function makeAllChangedFlags(count: number): Uint8Array {
  const flags = new Uint8Array(count);
  flags.fill(CHANGED_TRANSFORM);
  return flags;
}

/**
 * Sine-field motion: every cube sits on a 100×50 grid, each oscillating with
 * a per-cube phase derived from its grid coordinates. Cheap on the CPU side
 * and produces visibly distinct motion across the scene.
 */
function updateTransforms(
  transforms: Float32Array,
  count: number,
  time: number,
): void {
  const cols = 100;
  const rows = Math.ceil(count / cols);
  const spacing = 2.5;
  const halfX = (cols - 1) * 0.5 * spacing;
  const halfZ = (rows - 1) * 0.5 * spacing;
  for (let i = 0; i < count; i++) {
    const col = i % cols;
    const row = (i / cols) | 0;
    const off = i * TRANSFORM_STRIDE;
    const x = col * spacing - halfX;
    const z = row * spacing - halfZ;
    const y =
      Math.sin(time * 1.5 + col * 0.18) * 1.6 +
      Math.cos(time * 0.9 + row * 0.21) * 1.6;
    transforms[off + 0] = x;
    transforms[off + 1] = y;
    transforms[off + 2] = z;
  }
}

function attachHud(mode: Mode, entityCount: number): {
  setFps: (fps: number) => void;
} {
  const modeEl = document.getElementById("mode")!;
  const entitiesEl = document.getElementById("entities")!;
  const fpsEl = document.getElementById("fps")!;
  modeEl.textContent = mode;
  entitiesEl.textContent = entityCount.toString();

  return {
    setFps(fps: number) {
      fpsEl.textContent = fps.toFixed(1);
    },
  };
}

function createBoxGeometry(): THREE.BoxGeometry {
  return new THREE.BoxGeometry(1, 1, 1);
}

function createMaterial(): THREE.Material {
  // MeshLambertMaterial responds to lighting and respects per-instance
  // colors written through `instanceColor` — without us touching
  // `vertexColors`, three.js wires up the shader path automatically once
  // `mesh.instanceColor` is set.
  return new THREE.MeshLambertMaterial({ color: 0xffffff });
}

function main(): void {
  const mode = readMode();
  const { renderer, scene, camera } = setupScene();
  const cache = new RendererCache(scene);
  cache.registerGeometry(MESH_HANDLE_ID, createBoxGeometry());
  cache.registerMaterial(MATERIAL_HANDLE_ID, createMaterial());

  const buffers = allocPacketBuffers(ENTITY_COUNT, mode);
  const hud = attachHud(mode, ENTITY_COUNT);

  // Rolling-FPS state.
  const SAMPLE_FRAMES = 60;
  let frameTimes: number[] = [];
  let lastFrameMs = performance.now();
  let lastHudUpdateMs = lastFrameMs;

  const start = performance.now();
  function frame(): void {
    const now = performance.now();
    const dtMs = now - lastFrameMs;
    lastFrameMs = now;
    if (frameTimes.length >= SAMPLE_FRAMES) frameTimes.shift();
    frameTimes.push(dtMs);

    const t = (now - start) / 1000;
    updateTransforms(buffers.transforms, ENTITY_COUNT, t);
    const packet = buildPacket(buffers, ENTITY_COUNT);
    cache.applyFrame(packet);

    renderer.render(scene, camera);

    if (now - lastHudUpdateMs > 250) {
      const avgDt =
        frameTimes.reduce((a, b) => a + b, 0) / Math.max(1, frameTimes.length);
      hud.setFps(1000 / Math.max(0.0001, avgDt));
      lastHudUpdateMs = now;
    }

    globalThis.requestAnimationFrame(frame);
  }
  frame();
}

main();
