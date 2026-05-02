// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import type { PickingTarget } from "./picking.js";

export interface MarqueeOverlayTarget extends PickingTarget {
  readonly ownerDocument?: Document;
}

export interface MarqueeOverlayOptions {
  /**
   * CSS class applied to the transient overlay element.
   * Defaults to `galeon-marquee-overlay`.
   */
  readonly className?: string;
  /** Parent for the overlay element. Defaults to `canvas.ownerDocument.body`. */
  readonly container?: HTMLElement;
  /** CSS border for the rectangle. */
  readonly border?: string;
  /** CSS background for the rectangle fill. */
  readonly background?: string;
  /** CSS z-index for the rectangle. */
  readonly zIndex?: string;
}

const DEFAULT_CLASS_NAME = "galeon-marquee-overlay";
const DEFAULT_BORDER = "1px solid rgba(80, 160, 255, 0.95)";
const DEFAULT_BACKGROUND = "rgba(80, 160, 255, 0.16)";
const DEFAULT_Z_INDEX = "2147483647";

/**
 * Render a live CSS drag rectangle over the viewport while the left mouse
 * button is held on `canvas`.
 *
 * This is intentionally visual-only: it does not emit pick events, inspect the
 * Three.js scene, or mutate Rust selection state. Pair it with `attachPicking`
 * when an application wants both marquee selection behavior and a standard HUD
 * rectangle.
 */
export function attachMarqueeOverlay(
  canvas: MarqueeOverlayTarget,
  options: MarqueeOverlayOptions = {},
): () => void {
  let overlay: HTMLElement | null = null;
  let startClientX = 0;
  let startClientY = 0;

  function onMouseDown(event: MouseEvent): void {
    if (event.button !== 0) return;
    removeOverlay();
    startClientX = event.clientX;
    startClientY = event.clientY;
    overlay = createOverlay(canvas, options);
    updateOverlay(overlay, startClientX, startClientY, event.clientX, event.clientY);
  }

  function onMouseMove(event: MouseEvent): void {
    if (overlay == null) return;
    updateOverlay(overlay, startClientX, startClientY, event.clientX, event.clientY);
  }

  function onMouseUp(event: MouseEvent): void {
    if (event.button !== 0) return;
    removeOverlay();
  }

  function onMouseLeave(): void {
    removeOverlay();
  }

  function removeOverlay(): void {
    overlay?.remove();
    overlay = null;
  }

  canvas.addEventListener("mousedown", onMouseDown);
  canvas.addEventListener("mousemove", onMouseMove);
  canvas.addEventListener("mouseup", onMouseUp);
  canvas.addEventListener("mouseleave", onMouseLeave);

  return () => {
    removeOverlay();
    canvas.removeEventListener("mousedown", onMouseDown);
    canvas.removeEventListener("mousemove", onMouseMove);
    canvas.removeEventListener("mouseup", onMouseUp);
    canvas.removeEventListener("mouseleave", onMouseLeave);
  };
}

function createOverlay(
  canvas: MarqueeOverlayTarget,
  options: MarqueeOverlayOptions,
): HTMLElement {
  const document = canvas.ownerDocument ?? globalThis.document;
  const container = options.container ?? document.body;
  const element = document.createElement("div");
  element.className = options.className ?? DEFAULT_CLASS_NAME;
  element.style.position = "fixed";
  element.style.pointerEvents = "none";
  element.style.boxSizing = "border-box";
  element.style.border = options.border ?? DEFAULT_BORDER;
  element.style.background = options.background ?? DEFAULT_BACKGROUND;
  element.style.zIndex = options.zIndex ?? DEFAULT_Z_INDEX;
  container.appendChild(element);
  return element;
}

function updateOverlay(
  element: HTMLElement,
  startClientX: number,
  startClientY: number,
  currentClientX: number,
  currentClientY: number,
): void {
  const left = Math.min(startClientX, currentClientX);
  const top = Math.min(startClientY, currentClientY);
  const width = Math.abs(currentClientX - startClientX);
  const height = Math.abs(currentClientY - startClientY);
  element.style.left = `${left}px`;
  element.style.top = `${top}px`;
  element.style.width = `${width}px`;
  element.style.height = `${height}px`;
}
