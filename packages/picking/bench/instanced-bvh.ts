// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import {
  GALEON_INSTANCE_ENTITIES_KEY,
  RENDER_CONTRACT_VERSION,
  RendererCache,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  type FramePacketView,
  type GaleonInstancedMesh,
  type InstancedEntityResolver,
} from "@galeon/three";
import {
  attachPicking,
  createGaleonPickingBackend,
  type PickingBackend,
  type PickingEntityRef,
  type PickingEvent,
  type PickingPointHit,
  type PickingTarget,
} from "../src/picking.js";
import { frustumFromRect } from "../src/selection-frustum.js";

const ENTITY_COUNT = 10_000;
const SIDE = 100;
const WARMUP_ITERATIONS = 20;
const SAMPLE_ITERATIONS = 80;
const CANVAS_WIDTH = 800;
const CANVAS_HEIGHT = 600;
const CLICK_X = 404;
const CLICK_Y = 297;

class BenchCanvas implements PickingTarget {
  readonly listeners: Array<{ readonly type: string; readonly fn: (event: MouseEvent) => void }> = [];

  addEventListener(type: string, fn: (event: MouseEvent) => void): void {
    this.listeners.push({ type, fn });
  }

  removeEventListener(type: string, fn: (event: MouseEvent) => void): void {
    const index = this.listeners.findIndex((listener) => listener.type === type && listener.fn === fn);
    if (index >= 0) this.listeners.splice(index, 1);
  }

  getBoundingClientRect(): { left: number; top: number; width: number; height: number } {
    return { left: 0, top: 0, width: CANVAS_WIDTH, height: CANVAS_HEIGHT };
  }

  dispatch(type: string, event: Partial<MouseEvent>): void {
    const full = {
      button: 0,
      clientX: 0,
      clientY: 0,
      shiftKey: false,
      ctrlKey: false,
      altKey: false,
      metaKey: false,
      ...event,
    } as MouseEvent;
    for (const listener of this.listeners) {
      if (listener.type === type) listener.fn(full);
    }
  }
}

interface BenchRow {
  readonly backend: "linear" | "bvh";
  readonly operation: "click" | "marquee";
  readonly medianMs: number;
  readonly p95Ms: number;
  readonly selected: number;
}

function makeScene(): { scene: THREE.Scene; camera: THREE.OrthographicCamera } {
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-70, 70, 70, -70, 0.1, 200);
  camera.position.set(0, 0, 100);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld();

  const cache = new RendererCache(scene);
  cache.registerGeometry(7, new THREE.BoxGeometry(0.8, 0.8, 0.8));
  cache.registerMaterial(0, new THREE.MeshBasicMaterial());
  cache.applyFrame(makePacket());
  scene.updateMatrixWorld(true);
  return { scene, camera };
}

function makePacket(): FramePacketView {
  const packet: FramePacketView = {
    contract_version: RENDER_CONTRACT_VERSION,
    entity_count: ENTITY_COUNT,
    entity_ids: new Uint32Array(ENTITY_COUNT),
    entity_generations: new Uint32Array(ENTITY_COUNT),
    transforms: new Float32Array(ENTITY_COUNT * TRANSFORM_STRIDE),
    visibility: new Uint8Array(ENTITY_COUNT).fill(1),
    mesh_handles: new Uint32Array(ENTITY_COUNT).fill(7),
    material_handles: new Uint32Array(ENTITY_COUNT),
    parent_ids: new Uint32Array(ENTITY_COUNT).fill(SCENE_ROOT),
    instance_groups: new Uint32Array(ENTITY_COUNT).fill(7),
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
  };

  const spacing = 1.25;
  const offset = ((SIDE - 1) * spacing) / 2;
  for (let i = 0; i < ENTITY_COUNT; i++) {
    const off = i * TRANSFORM_STRIDE;
    packet.entity_ids[i] = i + 1;
    packet.transforms[off] = (i % SIDE) * spacing - offset;
    packet.transforms[off + 1] = Math.floor(i / SIDE) * spacing - offset;
    packet.transforms[off + 6] = 1;
    packet.transforms[off + 7] = 1;
    packet.transforms[off + 8] = 1;
    packet.transforms[off + 9] = 1;
  }
  return packet;
}

function runBackend(
  backendName: BenchRow["backend"],
  backend: PickingBackend,
): BenchRow[] {
  const { scene, camera } = makeScene();
  const canvas = new BenchCanvas();
  let lastEvent: PickingEvent | null = null;
  const dispose = attachPicking(canvas, scene, camera, {
    dragThreshold: 2,
    pickingBackend: backend,
    onPick: (event) => {
      lastEvent = event;
    },
  });

  const click = measure(backendName, "click", () => {
    canvas.dispatch("mousedown", { clientX: CLICK_X, clientY: CLICK_Y });
    canvas.dispatch("mouseup", { clientX: CLICK_X, clientY: CLICK_Y });
  }, () => (lastEvent?.kind === "pick" && lastEvent.entity != null ? 1 : 0));

  const marquee = measure(backendName, "marquee", () => {
    canvas.dispatch("mousedown", { clientX: 0, clientY: 0 });
    canvas.dispatch("mousemove", { clientX: CANVAS_WIDTH / 4, clientY: CANVAS_HEIGHT / 4 });
    canvas.dispatch("mouseup", { clientX: CANVAS_WIDTH / 4, clientY: CANVAS_HEIGHT / 4 });
  }, () => (lastEvent?.kind === "pick-rect" ? lastEvent.entities.length : 0));

  dispose();
  return [click, marquee];
}

function measure(
  backend: BenchRow["backend"],
  operation: BenchRow["operation"],
  dispatch: () => void,
  readSelected: () => number,
): BenchRow {
  for (let i = 0; i < WARMUP_ITERATIONS; i++) dispatch();
  const samples: number[] = [];
  for (let i = 0; i < SAMPLE_ITERATIONS; i++) {
    const start = performance.now();
    dispatch();
    samples.push(performance.now() - start);
  }
  samples.sort((a, b) => a - b);
  return {
    backend,
    operation,
    medianMs: percentile(samples, 0.5),
    p95Ms: percentile(samples, 0.95),
    selected: readSelected(),
  };
}

function createLinearInstancedBackend(): PickingBackend {
  const raycaster = new THREE.Raycaster();
  const frustum = new THREE.Frustum();
  const matrix = new THREE.Matrix4();
  const worldMatrix = new THREE.Matrix4();
  const probe = new THREE.Mesh();
  const intersections: THREE.Intersection[] = [];
  const box = new THREE.Box3();

  return {
    pickAt({ scene, camera, ndc }) {
      scene.updateMatrixWorld(true);
      camera.updateMatrixWorld();
      raycaster.layers.mask = camera.layers.mask;
      raycaster.setFromCamera(ndc, camera);
      let best: { readonly hit: PickingPointHit; readonly distance: number } | null = null;
      for (const mesh of instancedMeshes(scene)) {
        if (!mesh.layers.test(camera.layers)) continue;
        const resolver = resolverFor(mesh);
        if (resolver == null) continue;
        for (let i = 0; i < mesh.capacity; i++) {
          if (!mesh.getActiveAndVisibilityAt(i)) continue;
          const entity = resolver.entityAt(i);
          if (entity == null) continue;
          mesh.getMatrixAt(i, matrix);
          worldMatrix.multiplyMatrices(mesh.matrixWorld, matrix);
          probe.geometry = mesh.geometry;
          probe.material = Array.isArray(mesh.material) ? mesh.material[0]! : mesh.material;
          probe.matrixWorld.copy(worldMatrix);
          intersections.length = 0;
          probe.raycast(raycaster, intersections);
          for (const preciseHit of intersections) {
            const distance = preciseHit.point.distanceTo(raycaster.ray.origin);
            if (best != null && distance >= best.distance) continue;
            const p = preciseHit.point;
            best = {
              distance,
              hit: { entity, point: { x: p.x, y: p.y, z: p.z } },
            };
          }
        }
      }
      return best?.hit ?? null;
    },
    pickRect({ scene, camera, ndcStart, ndcEnd }) {
      scene.updateMatrixWorld(true);
      camera.updateMatrixWorld();
      frustum.copy(frustumFromRect(ndcStart, ndcEnd, camera));
      const out: PickingEntityRef[] = [];
      for (const mesh of instancedMeshes(scene)) {
        const resolver = resolverFor(mesh);
        if (resolver == null) continue;
        if (mesh.geometry.boundingBox === null) mesh.geometry.computeBoundingBox();
        if (mesh.geometry.boundingBox === null) continue;
        for (let i = 0; i < mesh.capacity; i++) {
          if (!mesh.getActiveAndVisibilityAt(i)) continue;
          const entity = resolver.entityAt(i);
          if (entity == null) continue;
          mesh.getMatrixAt(i, matrix);
          worldMatrix.multiplyMatrices(mesh.matrixWorld, matrix);
          box.copy(mesh.geometry.boundingBox).applyMatrix4(worldMatrix);
          if (frustum.intersectsBox(box)) out.push(entity);
        }
      }
      return out;
    },
  };
}

function instancedMeshes(scene: THREE.Scene): GaleonInstancedMesh[] {
  const out: GaleonInstancedMesh[] = [];
  scene.traverse((object) => {
    if ((object as { readonly isInstancedMesh2?: unknown }).isInstancedMesh2 === true) {
      out.push(object as GaleonInstancedMesh);
    }
  });
  return out;
}

function resolverFor(mesh: GaleonInstancedMesh): InstancedEntityResolver | undefined {
  const data = mesh.userData as Record<PropertyKey, unknown> | undefined;
  return data?.[GALEON_INSTANCE_ENTITIES_KEY] as InstancedEntityResolver | undefined;
}

function percentile(samples: readonly number[], p: number): number {
  if (samples.length === 0) return 0;
  const index = Math.min(samples.length - 1, Math.max(0, Math.ceil(samples.length * p) - 1));
  return samples[index] ?? 0;
}

function formatMs(value: number): string {
  return value.toFixed(3);
}

function main(): void {
  const rows = [
    ...runBackend("linear", createLinearInstancedBackend()),
    ...runBackend("bvh", createGaleonPickingBackend()),
  ];
  console.log("# @galeon/picking Instanced BVH Benchmark");
  console.log("");
  console.log(`Runtime: Bun ${process.versions.bun ?? "unknown"} / ${process.platform} ${process.arch}`);
  console.log(`Scene: ${ENTITY_COUNT.toLocaleString("en-US")} InstancedMesh2 cubes`);
  console.log(`Warmup: ${WARMUP_ITERATIONS} iterations per operation`);
  console.log(`Samples: ${SAMPLE_ITERATIONS} iterations per operation`);
  console.log("");
  console.log("| Backend | Operation | Median ms | P95 ms | Result size |");
  console.log("| --- | --- | ---: | ---: | ---: |");
  for (const row of rows) {
    console.log(
      `| ${row.backend} | ${row.operation} | ${formatMs(row.medianMs)} | ${formatMs(row.p95Ms)} | ${row.selected.toLocaleString("en-US")} |`,
    );
  }
}

main();
