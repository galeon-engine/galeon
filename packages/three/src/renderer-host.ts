// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";

export type RendererHostBackend = "webgl" | "webgpu" | (string & {});

export interface RendererHostFrame {
  readonly host: RendererHost;
  readonly timeMs: number;
  readonly deltaMs: number;
}

export interface RendererHostAdapter {
  readonly backend: RendererHostBackend;
  readonly renderer: unknown;
  readonly domElement: HTMLCanvasElement;
  render(scene: THREE.Scene, camera: THREE.Camera): void;
  setSize?(width: number, height: number, updateStyle?: boolean): void;
  setAnimationLoop?(callback: ((timeMs: number) => void) | null): void;
  dispose?(): void;
}

export interface RendererHostClock {
  requestFrame(callback: (timeMs: number) => void): unknown;
  cancelFrame(handle: unknown): void;
  now?(): number;
}

export interface RendererHostOptions {
  readonly adapter: RendererHostAdapter;
  readonly scene?: THREE.Scene;
  readonly camera?: THREE.Camera;
  readonly clock?: RendererHostClock;
  readonly autoRender?: boolean;
  readonly onFrame?: (frame: RendererHostFrame) => void;
}

let nextRendererHostId = 1;

/**
 * Owns renderer/canvas lifecycle independently from UI component state.
 *
 * UI shells can attach/detach the host canvas and subscribe to frames, but
 * ordinary UI state changes should not recreate this object or its renderer.
 */
export class RendererHost {
  readonly id = `galeon-renderer-host-${nextRendererHostId++}`;
  readonly scene: THREE.Scene;
  readonly camera: THREE.Camera;
  readonly adapter: RendererHostAdapter;

  private readonly clock: RendererHostClock;
  private readonly autoRender: boolean;
  private readonly onFrame?: (frame: RendererHostFrame) => void;
  private frameHandle: unknown;
  private running = false;
  private disposed = false;
  private lastTimeMs: number | undefined;
  private _frameCount = 0;

  constructor(options: RendererHostOptions) {
    this.adapter = options.adapter;
    this.scene = options.scene ?? new THREE.Scene();
    this.camera = options.camera ?? new THREE.PerspectiveCamera();
    this.clock = options.clock ?? browserFrameClock();
    this.autoRender = options.autoRender ?? true;
    this.onFrame = options.onFrame;
  }

  get canvas(): HTMLCanvasElement {
    return this.adapter.domElement;
  }

  get backend(): RendererHostBackend {
    return this.adapter.backend;
  }

  get rendererIdentity(): unknown {
    return this.adapter.renderer;
  }

  get isRunning(): boolean {
    return this.running;
  }

  get frameCount(): number {
    return this._frameCount;
  }

  attachTo(parent: HTMLElement): void {
    this.assertNotDisposed();
    if (this.canvas.parentElement === parent) {
      return;
    }
    this.canvas.parentElement?.removeChild(this.canvas);
    parent.appendChild(this.canvas);
  }

  detach(): void {
    this.canvas.parentElement?.removeChild(this.canvas);
  }

  setSize(width: number, height: number, updateStyle = true): void {
    this.assertNotDisposed();
    this.adapter.setSize?.(width, height, updateStyle);
  }

  start(): void {
    this.assertNotDisposed();
    if (this.running) {
      return;
    }
    this.running = true;
    this.lastTimeMs = undefined;

    if (this.adapter.setAnimationLoop !== undefined) {
      this.adapter.setAnimationLoop((timeMs) => this.tick(timeMs));
      return;
    }

    this.frameHandle = this.clock.requestFrame((timeMs) =>
      this.tick(timeMs),
    );
  }

  stop(): void {
    if (!this.running) {
      return;
    }
    this.running = false;

    if (this.adapter.setAnimationLoop !== undefined) {
      this.adapter.setAnimationLoop(null);
    }
    if (this.frameHandle !== undefined) {
      this.clock.cancelFrame(this.frameHandle);
      this.frameHandle = undefined;
    }
  }

  render(): void {
    this.assertNotDisposed();
    this.adapter.render(this.scene, this.camera);
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.stop();
    this.detach();
    this.adapter.dispose?.();
    this.disposed = true;
  }

  private tick(timeMs: number): void {
    if (!this.running) {
      return;
    }

    const deltaMs =
      this.lastTimeMs === undefined ? 0 : Math.max(0, timeMs - this.lastTimeMs);
    this.lastTimeMs = timeMs;
    this._frameCount += 1;
    this.onFrame?.({ host: this, timeMs, deltaMs });
    if (this.autoRender) {
      this.render();
    }

    if (this.running && this.adapter.setAnimationLoop === undefined) {
      this.frameHandle = this.clock.requestFrame((nextTimeMs) =>
        this.tick(nextTimeMs),
      );
    }
  }

  private assertNotDisposed(): void {
    if (this.disposed) {
      throw new Error("RendererHost has been disposed");
    }
  }
}

export function createThreeRendererHostAdapter(
  backend: RendererHostBackend,
  renderer: {
    readonly domElement: HTMLCanvasElement;
    render(scene: THREE.Scene, camera: THREE.Camera): void;
    setSize?(width: number, height: number, updateStyle?: boolean): void;
    setAnimationLoop?(callback: ((timeMs: number) => void) | null): void;
    dispose?(): void;
  },
): RendererHostAdapter {
  return {
    backend,
    renderer,
    domElement: renderer.domElement,
    render: (scene, camera) => renderer.render(scene, camera),
    setSize: renderer.setSize?.bind(renderer),
    setAnimationLoop: renderer.setAnimationLoop?.bind(renderer),
    dispose: renderer.dispose?.bind(renderer),
  };
}

function browserFrameClock(): RendererHostClock {
  const raf = globalThis.requestAnimationFrame;
  const caf = globalThis.cancelAnimationFrame;
  if (raf === undefined || caf === undefined) {
    throw new Error(
      "RendererHost requires a clock outside browser animation-frame environments",
    );
  }
  return {
    requestFrame: (callback) => raf(callback),
    cancelFrame: (handle) => caf(handle as number),
    now: () => performance.now(),
  };
}
