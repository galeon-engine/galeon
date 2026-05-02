// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { describe, expect, test } from "bun:test";
import {
  attachMarqueeOverlay,
  type MarqueeOverlayTarget,
} from "../src/marquee-overlay.js";

interface FakeListener {
  type: string;
  fn: (event: MouseEvent) => void;
}

class FakeStyle {
  position = "";
  pointerEvents = "";
  boxSizing = "";
  border = "";
  background = "";
  zIndex = "";
  left = "";
  top = "";
  width = "";
  height = "";
}

class FakeElement {
  readonly style = new FakeStyle() as CSSStyleDeclaration;
  readonly children: FakeElement[] = [];
  className = "";
  parent: FakeElement | null = null;

  appendChild(child: FakeElement): FakeElement {
    child.parent = this;
    this.children.push(child);
    return child;
  }

  remove(): void {
    if (this.parent == null) return;
    const index = this.parent.children.indexOf(this);
    if (index >= 0) this.parent.children.splice(index, 1);
    this.parent = null;
  }
}

class FakeDocument {
  readonly body = new FakeElement();

  createElement(_tag: string): FakeElement {
    return new FakeElement();
  }
}

class FakeCanvas implements MarqueeOverlayTarget {
  readonly ownerDocument = new FakeDocument() as unknown as Document;
  readonly listeners: FakeListener[] = [];
  rect = { left: 0, top: 0, width: 800, height: 600 };

  addEventListener(type: string, fn: (event: MouseEvent) => void): void {
    this.listeners.push({ type, fn });
  }

  removeEventListener(type: string, fn: (event: MouseEvent) => void): void {
    const index = this.listeners.findIndex((listener) => listener.type === type && listener.fn === fn);
    if (index >= 0) this.listeners.splice(index, 1);
  }

  getBoundingClientRect() {
    return this.rect;
  }

  fire(type: string, event: Partial<MouseEvent>): void {
    const full = { ...defaultMouseEvent(), ...event } as MouseEvent;
    for (const listener of this.listeners.filter((l) => l.type === type)) {
      listener.fn(full);
    }
  }

  body(): FakeElement {
    return (this.ownerDocument.body as unknown) as FakeElement;
  }
}

function defaultMouseEvent(): MouseEvent {
  return {
    button: 0,
    clientX: 0,
    clientY: 0,
  } as MouseEvent;
}

describe("attachMarqueeOverlay", () => {
  test("adds the overlay on mousedown and sizes it during mousemove", () => {
    const canvas = new FakeCanvas();
    attachMarqueeOverlay(canvas);

    canvas.fire("mousedown", { clientX: 20, clientY: 30 });
    expect(canvas.body().children).toHaveLength(1);
    const overlay = canvas.body().children[0]!;
    expect(overlay.className).toBe("galeon-marquee-overlay");
    expect(overlay.style.left).toBe("20px");
    expect(overlay.style.top).toBe("30px");
    expect(overlay.style.width).toBe("0px");
    expect(overlay.style.height).toBe("0px");

    canvas.fire("mousemove", { clientX: 120, clientY: 80 });

    expect(overlay.style.left).toBe("20px");
    expect(overlay.style.top).toBe("30px");
    expect(overlay.style.width).toBe("100px");
    expect(overlay.style.height).toBe("50px");
  });

  test("normalizes reverse drags", () => {
    const canvas = new FakeCanvas();
    attachMarqueeOverlay(canvas);

    canvas.fire("mousedown", { clientX: 120, clientY: 80 });
    canvas.fire("mousemove", { clientX: 20, clientY: 30 });

    const overlay = canvas.body().children[0]!;
    expect(overlay.style.left).toBe("20px");
    expect(overlay.style.top).toBe("30px");
    expect(overlay.style.width).toBe("100px");
    expect(overlay.style.height).toBe("50px");
  });

  test("removes the overlay on mouseup and dispose", () => {
    const canvas = new FakeCanvas();
    const dispose = attachMarqueeOverlay(canvas);

    canvas.fire("mousedown", { clientX: 20, clientY: 30 });
    canvas.fire("mousemove", { clientX: 120, clientY: 80 });
    canvas.fire("mouseup", { clientX: 120, clientY: 80 });

    expect(canvas.body().children).toHaveLength(0);

    canvas.fire("mousedown", { clientX: 40, clientY: 50 });
    expect(canvas.body().children).toHaveLength(1);
    dispose();
    expect(canvas.body().children).toHaveLength(0);
    expect(canvas.listeners).toHaveLength(0);
  });

  test("ignores non-left-button presses", () => {
    const canvas = new FakeCanvas();
    attachMarqueeOverlay(canvas);

    canvas.fire("mousedown", { button: 1, clientX: 20, clientY: 30 });

    expect(canvas.body().children).toHaveLength(0);
  });
});
