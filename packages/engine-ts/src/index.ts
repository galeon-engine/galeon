// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { RUNTIME_VERSION } from "@galeon/runtime";

export const ENGINE_TS_VERSION = "0.1.0";

export function runtimeVersion(): string {
  return RUNTIME_VERSION;
}

// Renderer cache — Three.js sync consumer.
export { RendererCache, GALEON_ENTITY_KEY } from "./renderer-cache.js";
export { type FramePacketView, TRANSFORM_STRIDE } from "./types.js";
