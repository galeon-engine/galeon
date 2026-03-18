// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { RUNTIME_VERSION } from "@galeon/runtime";

/** Engine TypeScript bindings version. */
export const ENGINE_TS_VERSION = "0.1.0";

/** Returns the runtime version this package was built against. */
export function runtimeVersion(): string {
  return RUNTIME_VERSION;
}
