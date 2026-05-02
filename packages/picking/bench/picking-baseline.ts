// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";
import { GALEON_ENTITY_KEY } from "../../three/src/renderer-cache.js";
import { attachPicking, type PickingEvent, type PickingTarget } from "../src/picking.js";

const ENTITY_COUNTS = [100, 1_000, 10_000] as const;
const WARMUP_ITERATIONS = 25;
const SAMPLE_ITERATIONS = 125;
const CANVAS_WIDTH = 800;
const CANVAS_HEIGHT = 600;

interface Listener {
  readonly type: string;
  readonly fn: (event: MouseEvent) => void;
}

class BenchCanvas implements PickingTarget {
  readonly listeners: Listener[] = [];

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
    const full = { ...defaultMouseEvent(), ...event } as MouseEvent;
    for (const listener of this.listeners) {
      if (listener.type === type) listener.fn(full);
    }
  }
}

interface BenchScene {
  readonly scene: THREE.Scene;
  readonly camera: THREE.OrthographicCamera;
}

interface BenchmarkRow {
  readonly entities: number;
  readonly operation: "click" | "marquee";
  readonly samples: number;
  readonly medianMs: number;
  readonly p95Ms: number;
  readonly minMs: number;
  readonly maxMs: number;
  readonly selected: number;
}

function defaultMouseEvent(): MouseEvent {
  return {
    button: 0,
    clientX: 0,
    clientY: 0,
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
  } as MouseEvent;
}

function stampEntity(object: THREE.Object3D, entityId: number): void {
  (object.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = {
    entityId,
    generation: 0,
  };
}

function makeStandaloneScene(entityCount: number): BenchScene {
  const scene = new THREE.Scene();
  const geometry = new THREE.BoxGeometry(0.6, 0.6, 0.6);
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  const material = new THREE.MeshBasicMaterial();

  const target = new THREE.Mesh(geometry, material);
  stampEntity(target, 1);
  scene.add(target);

  const side = Math.ceil(Math.sqrt(entityCount - 1));
  const spacing = 1.25;
  const offset = ((side - 1) * spacing) / 2;
  for (let i = 1; i < entityCount; i++) {
    const index = i - 1;
    let x = (index % side) * spacing - offset;
    const y = Math.floor(index / side) * spacing - offset;
    if (Math.abs(x) < spacing && Math.abs(y) < spacing) {
      x += side * spacing;
    }
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.set(x, y, -2);
    stampEntity(mesh, i + 1);
    scene.add(mesh);
  }

  const span = Math.max(4, side * spacing * 2 + 4);
  const aspect = CANVAS_WIDTH / CANVAS_HEIGHT;
  const camera = new THREE.OrthographicCamera(
    (-span * aspect) / 2,
    (span * aspect) / 2,
    span / 2,
    -span / 2,
    0.1,
    100,
  );
  camera.position.set(0, 0, 25);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld(true);
  scene.updateMatrixWorld(true);
  return { scene, camera };
}

function measureOperation(
  entities: number,
  operation: BenchmarkRow["operation"],
  dispatch: () => void,
  readSelected: () => number,
): BenchmarkRow {
  for (let i = 0; i < WARMUP_ITERATIONS; i++) dispatch();

  const samples: number[] = [];
  for (let i = 0; i < SAMPLE_ITERATIONS; i++) {
    const start = performance.now();
    dispatch();
    samples.push(performance.now() - start);
  }
  samples.sort((a, b) => a - b);
  return {
    entities,
    operation,
    samples: samples.length,
    medianMs: percentile(samples, 0.5),
    p95Ms: percentile(samples, 0.95),
    minMs: samples[0] ?? 0,
    maxMs: samples[samples.length - 1] ?? 0,
    selected: readSelected(),
  };
}

function percentile(sortedSamples: readonly number[], p: number): number {
  if (sortedSamples.length === 0) return 0;
  const index = Math.min(sortedSamples.length - 1, Math.max(0, Math.ceil(sortedSamples.length * p) - 1));
  return sortedSamples[index] ?? 0;
}

function runSceneBench(entityCount: number): BenchmarkRow[] {
  const { scene, camera } = makeStandaloneScene(entityCount);
  const canvas = new BenchCanvas();
  let lastEvent: PickingEvent | null = null;
  const dispose = attachPicking(canvas, scene, camera, {
    dragThreshold: 2,
    onPick: (event) => {
      lastEvent = event;
    },
  });

  const click = measureOperation(
    entityCount,
    "click",
    () => {
      canvas.dispatch("mousedown", { clientX: CANVAS_WIDTH / 2, clientY: CANVAS_HEIGHT / 2 });
      canvas.dispatch("mouseup", { clientX: CANVAS_WIDTH / 2, clientY: CANVAS_HEIGHT / 2 });
    },
    () => (lastEvent?.kind === "pick" && lastEvent.entity != null ? 1 : 0),
  );

  const marquee = measureOperation(
    entityCount,
    "marquee",
    () => {
      canvas.dispatch("mousedown", { clientX: 0, clientY: 0 });
      canvas.dispatch("mousemove", { clientX: CANVAS_WIDTH, clientY: CANVAS_HEIGHT });
      canvas.dispatch("mouseup", { clientX: CANVAS_WIDTH, clientY: CANVAS_HEIGHT });
    },
    () => (lastEvent?.kind === "pick-rect" ? lastEvent.entities.length : 0),
  );

  dispose();
  return [click, marquee];
}

function formatMs(value: number): string {
  return value.toFixed(3);
}

function main(): void {
  const rows = ENTITY_COUNTS.flatMap((count) => runSceneBench(count));
  console.log("# @galeon/picking Raycaster Baseline");
  console.log("");
  console.log(`Runtime: Bun ${process.versions.bun ?? "unknown"} / ${process.platform} ${process.arch}`);
  console.log(`Warmup: ${WARMUP_ITERATIONS} iterations per operation`);
  console.log(`Samples: ${SAMPLE_ITERATIONS} iterations per operation`);
  console.log("");
  console.log("| Entities | Operation | Median ms | P95 ms | Min ms | Max ms | Result size |");
  console.log("| ---: | --- | ---: | ---: | ---: | ---: | ---: |");
  for (const row of rows) {
    console.log(
      `| ${row.entities.toLocaleString("en-US")} | ${row.operation} | ${formatMs(row.medianMs)} | ${formatMs(row.p95Ms)} | ${formatMs(row.minMs)} | ${formatMs(row.maxMs)} | ${row.selected.toLocaleString("en-US")} |`,
    );
  }
}

main();
