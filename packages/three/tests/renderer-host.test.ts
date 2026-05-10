// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  RendererHost,
  createThreeRendererHostAdapter,
  type RendererHostClock,
} from "../src/index.js";

class ManualClock implements RendererHostClock {
  private callbacks = new Map<number, (timeMs: number) => void>();
  private nextHandle = 1;

  requestFrame(callback: (timeMs: number) => void): number {
    const handle = this.nextHandle++;
    this.callbacks.set(handle, callback);
    return handle;
  }

  cancelFrame(handle: unknown): void {
    this.callbacks.delete(handle as number);
  }

  flush(timeMs: number): void {
    const callbacks = Array.from(this.callbacks.values());
    this.callbacks.clear();
    for (const callback of callbacks) {
      callback(timeMs);
    }
  }
}

function makeRenderer() {
  const calls = {
    render: 0,
    dispose: 0,
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
});
