// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

export {
  GALEON_ENTITY_KEY,
  RendererCache,
  type RendererEntityHandle,
} from "./renderer-cache.js";

export { InstancedMeshManager } from "./instanced-mesh-manager.js";

export {
  CHANGED_INSTANCE_GROUP,
  CHANGED_MATERIAL,
  CHANGED_MESH,
  CHANGED_OBJECT_TYPE,
  CHANGED_PARENT,
  CHANGED_TRANSFORM,
  CHANGED_VISIBILITY,
  FramePacketContractError,
  INSTANCE_GROUP_NONE,
  ObjectType,
  RENDER_CONTRACT_VERSION,
  SCENE_ROOT,
  TRANSFORM_STRIDE,
  assertFramePacketContract,
  hasIncrementalChangeFlags,
  type FramePacketContractOptions,
  type FramePacketView,
} from "@galeon/render-core";
