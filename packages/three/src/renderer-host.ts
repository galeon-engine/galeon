// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import * as THREE from "three";

export type RendererHostBackend = "webgl" | "webgpu" | (string & {});

/**
 * Immutable per-frame context exposed to `onFrame` subscribers.
 */
export interface RendererHostFrame {
  readonly hostId: string;
  readonly backend: RendererHostBackend;
  readonly timeMs: number;
  readonly deltaMs: number;
  readonly frameCount: number;
}

export type RendererHostErrorPhase = "onFrame" | "render";

/**
 * Error context for failures that happen during the animation tick.
 */
export interface RendererHostErrorContext extends RendererHostFrame {
  readonly phase: RendererHostErrorPhase;
}

/**
 * Error callback for tick-level `onFrame` and `render` failures.
 */
export type RendererHostErrorHandler = (
  error: unknown,
  context: RendererHostErrorContext,
) => void;

/**
 * Minimal renderer surface used by the host.
 */
export interface RendererHostRenderer {
  readonly domElement: HTMLCanvasElement;
  render(scene: THREE.Scene, camera: THREE.Camera): void;
  setSize?(width: number, height: number, updateStyle?: boolean): void;
  setAnimationLoop?(callback: ((timeMs: number) => void) | null): void;
  dispose?(): void;
}

/**
 * Host adapter around WebGL/WebGPU renderers.
 */
export interface RendererHostAdapter<
  TRenderer extends RendererHostRenderer = RendererHostRenderer,
> {
  readonly backend: RendererHostBackend;
  readonly renderer: TRenderer;
  readonly domElement: HTMLCanvasElement;
  render(scene: THREE.Scene, camera: THREE.Camera): void;
  setSize?(width: number, height: number, updateStyle?: boolean): void;
  setAnimationLoop?(callback: ((timeMs: number) => void) | null): void;
  dispose?(): void;
}

/**
 * Animation-frame clock abstraction for browser and tests.
 */
export interface RendererHostClock<TFrameHandle = number> {
  requestFrame(callback: (timeMs: number) => void): TFrameHandle;
  cancelFrame(handle: TFrameHandle): void;
  now?(): number;
}

/**
 * Renderer host configuration.
 */
export interface RendererHostOptions<
  TRenderer extends RendererHostRenderer = RendererHostRenderer,
  TFrameHandle = number,
> {
  readonly adapter: RendererHostAdapter<TRenderer>;
  readonly scene?: THREE.Scene;
  readonly camera?: THREE.Camera;
  readonly clock?: RendererHostClock<TFrameHandle>;
  readonly autoRender?: boolean;
  readonly onFrame?: (frame: RendererHostFrame) => void;
  /**
   * Called when `onFrame` or `adapter.render` throws during a tick.
   *
   * The host keeps running after reporting the error unless user code
   * explicitly calls `stop()` or `dispose()`.
   */
  readonly onError?: RendererHostErrorHandler;
}

const RENDERER_HOST_ID_PREFIX = "galeon-renderer-host";
let fallbackRendererHostId = 1;

/**
 * Owns renderer/canvas lifecycle independently from UI component state.
 *
 * UI shells can attach/detach the host canvas and subscribe to frames, but
 * ordinary UI state changes should not recreate this object or its renderer.
 */
export class RendererHost<
  TRenderer extends RendererHostRenderer = RendererHostRenderer,
  TFrameHandle = number,
> {
  readonly id = createRendererHostId();
  readonly scene: THREE.Scene;
  readonly camera: THREE.Camera;
  readonly adapter: RendererHostAdapter<TRenderer>;

  private readonly clock?: RendererHostClock<TFrameHandle>;
  private activeClock?: RendererHostClock<TFrameHandle>;
  private readonly autoRender: boolean;
  private readonly onFrame?: (frame: RendererHostFrame) => void;
  private readonly onError: RendererHostErrorHandler;
  private frameHandle: TFrameHandle | undefined;
  private running = false;
  private disposed = false;
  private lastTimeMs: number | undefined;
  private _frameCount = 0;

  /**
   * Create a renderer host around a specific renderer adapter.
   */
  constructor(options: RendererHostOptions<TRenderer, TFrameHandle>) {
    this.adapter = options.adapter;
    this.scene = options.scene ?? new THREE.Scene();
    this.camera = options.camera ?? new THREE.PerspectiveCamera();
    this.clock = options.clock;
    this.autoRender = options.autoRender ?? true;
    this.onFrame = options.onFrame;
    this.onError = options.onError ?? defaultRendererHostErrorHandler;
  }

  /**
   * Canvas owned by this host.
   */
  get canvas(): HTMLCanvasElement {
    return this.adapter.domElement;
  }

  /**
   * Backend identity declared by the adapter.
   */
  get backend(): RendererHostBackend {
    return this.adapter.backend;
  }

  /**
   * Stable adapter renderer instance identity.
   */
  get rendererIdentity(): TRenderer {
    return this.adapter.renderer;
  }

  /**
   * Indicates whether the tick loop is active.
   */
  get isRunning(): boolean {
    return this.running;
  }

  /**
   * Number of processed frames since host construction.
   */
  get frameCount(): number {
    return this._frameCount;
  }

  /**
   * Attach the host canvas to a parent DOM node.
   */
  attachTo(parent: HTMLElement): void {
    this.assertNotDisposed();
    if (this.canvas.parentElement === parent) {
      return;
    }
    if (!parent.isConnected) {
      console.warn(
        `[RendererHost:${this.id}] attachTo() received a detached parent; canvas remains off-document until the parent is connected`,
      );
    }
    this.canvas.parentElement?.removeChild(this.canvas);
    parent.appendChild(this.canvas);
  }

  /**
   * Remove the host canvas from its current parent.
   */
  detach(): void {
    this.canvas.parentElement?.removeChild(this.canvas);
  }

  /**
   * Resize the renderer surface.
   */
  setSize(width: number, height: number, updateStyle = true): void {
    this.assertNotDisposed();
    this.adapter.setSize?.(width, height, updateStyle);
  }

  /**
   * Start the renderer tick loop.
   */
  start(): void {
    this.assertNotDisposed();
    if (this.running) {
      return;
    }
    this.running = true;
    this.lastTimeMs = undefined;

    try {
      if (this.adapter.setAnimationLoop !== undefined) {
        this.adapter.setAnimationLoop((timeMs) => this.tick(timeMs));
        return;
      }

      this.activeClock = this.resolveClock();
      this.frameHandle = this.activeClock.requestFrame((timeMs) =>
        this.tick(timeMs),
      );
    } catch (error) {
      this.running = false;
      this.activeClock = undefined;
      this.frameHandle = undefined;
      this.lastTimeMs = undefined;
      throw error;
    }
  }

  /**
   * Stop the renderer tick loop.
   */
  stop(): void {
    if (!this.running) {
      return;
    }
    this.running = false;

    if (this.adapter.setAnimationLoop !== undefined) {
      this.adapter.setAnimationLoop(null);
    }
    if (this.frameHandle !== undefined) {
      this.activeClock?.cancelFrame(this.frameHandle);
      this.frameHandle = undefined;
    }
    this.activeClock = undefined;
  }

  /**
   * Render the current scene once.
   */
  render(): void {
    this.assertNotDisposed();
    this.adapter.render(this.scene, this.camera);
  }

  /**
   * Stop the loop, detach the canvas, and dispose adapter resources.
   */
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
    if (!this.running || this.disposed) {
      return;
    }

    const deltaMs =
      this.lastTimeMs === undefined ? 0 : Math.max(0, timeMs - this.lastTimeMs);
    this.lastTimeMs = timeMs;
    this._frameCount += 1;
    const frame = this.createFrame(timeMs, deltaMs);

    if (this.onFrame !== undefined) {
      try {
        this.onFrame(frame);
      } catch (error) {
        this.reportError(error, "onFrame", frame);
      }
    }

    if (!this.running || this.disposed) {
      return;
    }

    if (this.autoRender) {
      try {
        this.render();
      } catch (error) {
        this.reportError(error, "render", frame);
      }
    }

    if (
      this.running &&
      !this.disposed &&
      this.adapter.setAnimationLoop === undefined
    ) {
      const clock = this.activeClock ?? this.resolveClock();
      this.activeClock = clock;
      this.frameHandle = clock.requestFrame((nextTimeMs) =>
        this.tick(nextTimeMs),
      );
    }
  }

  private resolveClock(): RendererHostClock<TFrameHandle> {
    return (this.clock ?? browserFrameClock()) as RendererHostClock<TFrameHandle>;
  }

  private createFrame(timeMs: number, deltaMs: number): RendererHostFrame {
    return {
      hostId: this.id,
      backend: this.backend,
      timeMs,
      deltaMs,
      frameCount: this._frameCount,
    };
  }

  private reportError(
    error: unknown,
    phase: RendererHostErrorPhase,
    frame: RendererHostFrame,
  ): void {
    try {
      this.onError(error, { ...frame, phase });
    } catch (handlerError) {
      defaultRendererHostErrorHandler(handlerError, { ...frame, phase });
    }
  }

  private assertNotDisposed(): void {
    if (this.disposed) {
      throw new Error("RendererHost has been disposed");
    }
  }
}

export function createThreeRendererHostAdapter<
  TRenderer extends RendererHostRenderer,
>(
  backend: RendererHostBackend,
  renderer: TRenderer,
): RendererHostAdapter<TRenderer> {
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

function browserFrameClock(): RendererHostClock<number> {
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

function createRendererHostId(): string {
  const randomUuid = globalThis.crypto?.randomUUID;
  if (typeof randomUuid === "function") {
    return `${RENDERER_HOST_ID_PREFIX}-${randomUuid.call(globalThis.crypto)}`;
  }
  return `${RENDERER_HOST_ID_PREFIX}-${fallbackRendererHostId++}`;
}

function defaultRendererHostErrorHandler(
  error: unknown,
  context: RendererHostErrorContext,
): void {
  console.error(
    `[RendererHost:${context.hostId}] ${context.phase} failed on frame ${context.frameCount} (${context.backend}) at ${context.timeMs}ms (delta ${context.deltaMs}ms)`,
    error,
  );
}
