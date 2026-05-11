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
    setSizeArgs: [] as Array<[number, number, boolean | undefined]>,
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
      setSize: (width: number, height: number, updateStyle?: boolean) => {
        calls.setSize += 1;
        calls.setSizeArgs.push([width, height, updateStyle]);
      },
    },
  };
}

function createMockCanvas(): HTMLCanvasElement {
  return {
    parentElement: null as HTMLElement | null,
  } as HTMLCanvasElement;
}

function createMockParent(options?: { isConnected?: boolean; removeError?: Error }) {
  const children: HTMLCanvasElement[] = [];
  let appendCalls = 0;
  let removeCalls = 0;

  const parent = {
    isConnected: options?.isConnected ?? true,
    appendChild: (child: HTMLCanvasElement) => {
      appendCalls += 1;
      (child as { parentElement: HTMLElement | null }).parentElement =
        parent as unknown as HTMLElement;
      children.push(child);
      return child;
    },
    removeChild: (child: HTMLCanvasElement) => {
      removeCalls += 1;
      if (options?.removeError !== undefined) {
        throw options.removeError;
      }
      const index = children.indexOf(child);
      if (index >= 0) {
        children.splice(index, 1);
      }
      (child as { parentElement: HTMLElement | null }).parentElement = null;
      return child;
    },
  };

  return {
    parent: parent as unknown as HTMLElement,
    children,
    get appendCalls(): number {
      return appendCalls;
    },
    get removeCalls(): number {
      return removeCalls;
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

  test("attachTo and detach move the canvas across parents without duplicates", () => {
    const canvas = createMockCanvas();
    const parentA = createMockParent();
    const parentB = createMockParent();
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgl", {
        domElement: canvas,
        render: () => {},
      }),
    });

    host.attachTo(parentA.parent);
    expect(parentA.children).toEqual([canvas]);
    expect(parentA.appendCalls).toBe(1);
    expect(parentA.removeCalls).toBe(0);

    host.attachTo(parentA.parent);
    expect(parentA.children).toEqual([canvas]);
    expect(parentA.appendCalls).toBe(1);
    expect(parentA.removeCalls).toBe(0);

    host.attachTo(parentB.parent);
    expect(parentA.children).toEqual([]);
    expect(parentA.removeCalls).toBe(1);
    expect(parentB.children).toEqual([canvas]);
    expect(parentB.appendCalls).toBe(1);

    host.detach();
    expect(parentB.children).toEqual([]);
    expect(parentB.removeCalls).toBe(1);
  });

  test("setSize forwards width, height, and updateStyle", () => {
    const { calls, renderer } = makeRenderer();
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgpu", renderer),
    });

    host.setSize(320, 200, false);
    host.setSize(640, 480);

    expect(calls.setSize).toBe(2);
    expect(calls.setSizeArgs).toEqual([
      [320, 200, false],
      [640, 480, true],
    ]);
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

  test("stop and start inside onFrame keep a single pending frame", () => {
    const clock = new ManualClock();
    const { calls, renderer } = makeRenderer();
    let restarted = false;
    let host!: RendererHost<typeof renderer, number>;
    host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgl", renderer),
      clock,
      onFrame: () => {
        if (!restarted) {
          restarted = true;
          host.stop();
          host.start();
        }
      },
    });

    host.start();
    expect(clock.pendingCount()).toBe(1);
    clock.flush(0);
    expect(clock.pendingCount()).toBe(1);
    clock.flush(16);

    expect(host.frameCount).toBe(2);
    expect(calls.render).toBe(2);

    host.stop();
    expect(clock.pendingCount()).toBe(0);
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

  test("onError handler failures still report the original tick error", () => {
    const clock = new ManualClock();
    const { renderer } = makeRenderer();
    const renderFailure = new Error("render failure");
    const handlerFailure = new Error("handler failure");
    renderer.render = () => {
      throw renderFailure;
    };
    const logs: unknown[][] = [];
    const originalConsoleError = console.error;
    console.error = (...args: unknown[]) => {
      logs.push(args);
    };

    try {
      const host = new RendererHost({
        adapter: createThreeRendererHostAdapter("webgpu", renderer),
        clock,
        onError: () => {
          throw handlerFailure;
        },
      });

      host.start();
      clock.flush(0);
      host.stop();

      expect(logs).toHaveLength(2);
      expect(logs[0]?.[1]).toBe(renderFailure);
      expect(logs[1]?.[0]).toContain(
        "onError handler failed while reporting render error",
      );
      expect(logs[1]?.[1]).toBe(handlerFailure);
    } finally {
      console.error = originalConsoleError;
    }
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

  test("fallback clock start can recover once animation-frame globals are restored", () => {
    const globals = globalThis as unknown as Record<string, unknown>;
    const originalRequestAnimationFrame = globals.requestAnimationFrame;
    const originalCancelAnimationFrame = globals.cancelAnimationFrame;
    const scheduled = new Map<number, (timeMs: number) => void>();
    let nextHandle = 1;

    try {
      const { calls, renderer } = makeRenderer();
      const host = new RendererHost({
        adapter: createThreeRendererHostAdapter("webgl", renderer),
      });

      globals.requestAnimationFrame = undefined;
      globals.cancelAnimationFrame = undefined;
      expect(() => host.start()).toThrow(
        "RendererHost requires a clock outside browser animation-frame environments",
      );
      expect(host.isRunning).toBe(false);

      globals.requestAnimationFrame = (callback: (timeMs: number) => void) => {
        const handle = nextHandle++;
        scheduled.set(handle, callback);
        return handle;
      };
      globals.cancelAnimationFrame = (handle: number) => {
        scheduled.delete(handle);
      };

      host.start();
      expect(host.isRunning).toBe(true);
      expect(scheduled.size).toBe(1);

      const handle = scheduled.keys().next().value as number | undefined;
      expect(handle).toBeDefined();
      const callback = scheduled.get(handle!);
      scheduled.delete(handle!);
      callback?.(12);

      expect(host.frameCount).toBe(1);
      expect(calls.render).toBe(1);

      host.stop();
      expect(host.isRunning).toBe(false);
      expect(scheduled.size).toBe(0);
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

  test("failed setAnimationLoop rollback warns and preserves the original start failure", () => {
    const { renderer } = makeRenderer();
    const startFailure = new Error("setAnimationLoop failed");
    const rollbackFailure = new Error("setAnimationLoop rollback failed");
    const callbacks: Array<((timeMs: number) => void) | null> = [];
    const warnings: unknown[][] = [];
    const originalConsoleWarn = console.warn;
    console.warn = (...args: unknown[]) => {
      warnings.push(args);
    };

    try {
      const host = new RendererHost({
        adapter: createThreeRendererHostAdapter("webgl", {
          ...renderer,
          setAnimationLoop: (callback) => {
            callbacks.push(callback);
            if (callback === null) {
              throw rollbackFailure;
            }
            throw startFailure;
          },
        }),
      });

      expect(() => host.start()).toThrow(startFailure);
      expect(host.isRunning).toBe(false);
      expect(callbacks).toHaveLength(2);
      expect(typeof callbacks[0]).toBe("function");
      expect(callbacks[1]).toBeNull();
      expect(warnings).toHaveLength(1);
      expect(warnings[0]?.[0]).toContain(
        "start() rollback failed while clearing animation loop",
      );
      expect(warnings[0]?.[1]).toBe(rollbackFailure);
    } finally {
      console.warn = originalConsoleWarn;
    }
  });

  test("dispose attempts stop, detach, and adapter disposal when cleanup throws", () => {
    const stopFailure = new Error("stop failed");
    const detachFailure = new Error("detach failed");
    const disposeFailure = new Error("dispose failed");
    const canvas = createMockCanvas();
    const parent = createMockParent({ removeError: detachFailure });
    let disposeCalls = 0;
    const setAnimationLoopCalls: Array<((timeMs: number) => void) | null> = [];
    const host = new RendererHost({
      adapter: createThreeRendererHostAdapter("webgpu", {
        domElement: canvas,
        render: () => {},
        setAnimationLoop: (callback) => {
          setAnimationLoopCalls.push(callback);
          if (callback === null) {
            throw stopFailure;
          }
        },
        dispose: () => {
          disposeCalls += 1;
          throw disposeFailure;
        },
      }),
    });

    host.attachTo(parent.parent);
    host.start();
    expect(() => host.dispose()).toThrow(stopFailure);

    expect(setAnimationLoopCalls).toHaveLength(2);
    expect(typeof setAnimationLoopCalls[0]).toBe("function");
    expect(setAnimationLoopCalls[1]).toBeNull();
    expect(parent.removeCalls).toBe(1);
    expect(disposeCalls).toBe(1);
    expect(host.isRunning).toBe(false);
    expect(() => host.start()).toThrow("RendererHost has been disposed");

    // Already marked disposed even if cleanup threw.
    expect(() => host.dispose()).not.toThrow();
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
