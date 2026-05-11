// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  RendererHost,
  createThreeRendererHostAdapter,
  type RendererHostClock,
} from "../src/index.js";

class ManualClock implements RendererHostClock<number> {
  private callbacks = new Map<number, (timeMs: number) => void>();
  private nextHandle = 1;

  requestFrame(callback: (timeMs: number) => void): number {
    const handle = this.nextHandle++;
    this.callbacks.set(handle, callback);
    return handle;
  }

  cancelFrame(handle: number): void {
    this.callbacks.delete(handle);
  }

  flush(timeMs: number): void {
    const callbacks = Array.from(this.callbacks.values());
    this.callbacks.clear();
    for (const callback of callbacks) {
      callback(timeMs);
    }
  }

  pendingCount(): number {
    return this.callbacks.size;
  }
}

function makeRenderer() {
  const calls = {
    render: 0,
    dispose: 0,
    setSize: 0,
  };
  return {
    calls,
    renderer: {
      domElement: {} as HTMLCanvasElement,
      render: (_scene: THREE.Scene, _camera: THREE.Camera) => {
        calls.render += 1;
      },
      dispose: () => {
        calls.dispose += 1;
      },
      setSize: () => {
        calls.setSize += 1;
      },
    },
  };
}

describe("RendererHost", () => {
  test("keeps stable renderer identity across start and stop", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgl", renderer),
      clock,
    });

    const identity = host.rendererIdentity;
    host.start();
    clock.flush(0);
    clock.flush(16);
    host.stop();
    host.start();
    clock.flush(32);

    expect(host.rendererIdentity).toBe(identity);
    expect(host.frameCount).toBe(3);
    expect(calls.render).toBe(3);
  });

  test("dispose stops the loop and disposes the renderer once", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgpu", renderer),
      clock,
    });

    host.start();
    host.dispose();
    clock.flush(0);
    host.dispose();

    expect(host.isRunning).toBe(false);
    expect(host.frameCount).toBe(0);
    expect(calls.render).toBe(0);
    expect(calls.dispose).toBe(1);
  });

  test("dispose during onFrame does not render or reschedule", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    let host!: RendererHost<typeof renderer, number>;
    host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgl", renderer),
      clock,
      onFrame: () => {
        host.dispose();
      },
    });

    host.start();
    clock.flush(5);
    clock.flush(10);

    expect(host.isRunning).toBe(false);
    expect(host.frameCount).toBe(1);
    expect(clock.pendingCount()).toBe(0);
    expect(calls.render).toBe(0);
    expect(calls.dispose).toBe(1);
  });

  test("reports onFrame errors and keeps the loop running", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    const failure = new Error("frame failure");
    const errors: Array<{
      error: unknown;
      phase: string;
      frameCount: number;
      backend: string;
    }> = [];

    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgpu", renderer),
      clock,
      onFrame: () => {
        throw failure;
      },
      onError: (error, context) => {
        errors.push({
          error,
          phase: context.phase,
          frameCount: context.frameCount,
          backend: context.backend,
        });
      },
    });

    host.start();
    clock.flush(0);
    clock.flush(16);
    host.stop();

    expect(host.frameCount).toBe(2);
    expect(calls.render).toBe(2);
    expect(errors).toEqual([
      { error: failure, phase: "onFrame", frameCount: 1, backend: "webgpu" },
      { error: failure, phase: "onFrame", frameCount: 2, backend: "webgpu" },
    ]);
  });

  test("reports render errors and keeps the loop running", () => {
    const clock = new ManualClock();
    const { renderer } = makeRenderer();
    const failure = new Error("render failure");
    renderer.render = () => {
      throw failure;
    };
    const errors: Array<{
      error: unknown;
      phase: string;
      frameCount: number;
    }> = [];

    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgl", renderer),
      clock,
      onError: (error, context) => {
        errors.push({
          error,
          phase: context.phase,
          frameCount: context.frameCount,
        });
      },
    });

    host.start();
    clock.flush(0);
    clock.flush(16);

    expect(host.isRunning).toBe(true);
    expect(host.frameCount).toBe(2);
    expect(errors).toEqual([
      { error: failure, phase: "render", frameCount: 1 },
      { error: failure, phase: "render", frameCount: 2 },
    ]);

    host.stop();
  });

  test("start wires setAnimationLoop callback and stop clears it", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    const callbacks: Array<((timeMs: number) => void) | null> = [];

    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgl", {
        ...renderer,
        setAnimationLoop: (callback) => {
          callbacks.push(callback);
        },
      }),
      clock,
    });

    host.start();
    expect(callbacks).toHaveLength(1);
    expect(typeof callbacks[0]).toBe("function");
    callbacks[0]?.(0);

    expect(host.frameCount).toBe(1);
    expect(calls.render).toBe(1);

    host.stop();
    expect(callbacks).toHaveLength(2);
    expect(callbacks[1]).toBeNull();
  });

  test("setAnimationLoop adapters do not require browser frame globals", () => {
    const globals = globalThis as unknown as Record<string, unknown>;
    const originalRequestAnimationFrame = globals.requestAnimationFrame;
    const originalCancelAnimationFrame = globals.cancelAnimationFrame;
    globals.requestAnimationFrame = undefined;
    globals.cancelAnimationFrame = undefined;

    try {
      const { calls, renderer } = makeRenderer();
      let callback: ((timeMs: number) => void) | null = null;

      const host = new RendererHost({
        adapter: createThreeRendererHostAdapter("webgpu", {
          ...renderer,
          setAnimationLoop: (nextCallback) => {
            callback = nextCallback;
          },
        }),
      });

      host.start();
      callback?.(8);

      expect(host.frameCount).toBe(1);
      expect(calls.render).toBe(1);

      host.stop();
      expect(callback).toBeNull();
    } finally {
      globals.requestAnimationFrame = originalRequestAnimationFrame;
      globals.cancelAnimationFrame = originalCancelAnimationFrame;
    }
  });

  test("failed fallback clock lookup leaves host stopped", () => {
    const globals = globalThis as unknown as Record<string, unknown>;
    const originalRequestAnimationFrame = globals.requestAnimationFrame;
    const originalCancelAnimationFrame = globals.cancelAnimationFrame;
    globals.requestAnimationFrame = undefined;
    globals.cancelAnimationFrame = undefined;

    try {
      const { calls, renderer } = makeRenderer();
      const host = new RendererHost({
        adapter: createThreeRendererHostAdapter("webgl", renderer),
      });

      expect(() => host.start()).toThrow(
        "RendererHost requires a clock outside browser animation-frame environments",
      );
      expect(host.isRunning).toBe(false);
      expect(host.frameCount).toBe(0);
      expect(calls.render).toBe(0);
    } finally {
      globals.requestAnimationFrame = originalRequestAnimationFrame;
      globals.cancelAnimationFrame = originalCancelAnimationFrame;
    }
  });

  test("failed setAnimationLoop start clears the renderer loop", () => {
    const { calls, renderer } = makeRenderer();
    const failure = new Error("setAnimationLoop failed");
    const callbacks: Array<((timeMs: number) => void) | null> = [];
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgpu", {
        ...renderer,
        setAnimationLoop: (callback) => {
          callbacks.push(callback);
          if (callback !== null) {
            throw failure;
          }
        },
      }),
    });

    expect(() => host.start()).toThrow(failure);
    expect(host.isRunning).toBe(false);
    expect(host.frameCount).toBe(0);
    expect(calls.render).toBe(0);
    expect(callbacks).toHaveLength(2);
    expect(typeof callbacks[0]).toBe("function");
    expect(callbacks[1]).toBeNull();
  });

  test("post-dispose APIs that require active lifecycle throw", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgpu", renderer),
      clock,
    });

    host.dispose();

    expect(() => host.start()).toThrow("RendererHost has been disposed");
    expect(() => host.render()).toThrow("RendererHost has been disposed");
    expect(() => host.setSize(320, 200)).toThrow(
      "RendererHost has been disposed",
    );
    expect(calls.setSize).toBe(0);
    expect(calls.dispose).toBe(1);
  });
});
