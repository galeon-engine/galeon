// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

export {
  attachMarqueeRenderer,
  type MarqueeRect,
  type MarqueeRendererController,
  type MarqueeRendererOptions,
} from "./marquee-renderer.js";

export {
  attachPicking,
  createGaleonPickingBackend,
  createRaycasterPickingBackend,
  type PickAtRequest,
  type PickingEntityRef,
  type PickingBackend,
  type PickingBackendOption,
  type PickingBackendSelection,
  type PickingEvent,
  type PickingFilter,
  type PickingOptions,
  type PickingPointHit,
  type PickingTarget,
  type PickEvent,
  type PickModifiers,
  type PickRectRequest,
  type PickRectEvent,
} from "./picking.js";

export {
  attachSelectionRings,
  type SelectionRingObjectResolver,
  type SelectionRingsController,
  type SelectionRingsOptions,
} from "./selection-rings.js";

export { frustumFromRect } from "./selection-frustum.js";
