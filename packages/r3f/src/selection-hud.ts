// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { useFrame, useThree } from "@react-three/fiber";
import {
  attachMarqueeOverlay,
  attachSelectionRings,
  type MarqueeOverlayOptions,
  type MarqueeOverlayTarget,
  type PickingEntityRef,
  type SelectionRingsController,
  type SelectionRingsOptions,
} from "@galeon/picking";
import {
  useLayoutEffect,
  useMemo,
  useRef,
} from "react";
import { useGaleonRendererCache } from "./provider.js";

export interface MarqueeOverlayProps extends MarqueeOverlayOptions {
  readonly canvas?: MarqueeOverlayTarget | null;
}

export function MarqueeOverlay({
  canvas,
  className,
  container,
  border,
  background,
  zIndex,
}: MarqueeOverlayProps) {
  const defaultCanvas = useThree((state) => state.gl.domElement as MarqueeOverlayTarget);
  const target = canvas ?? defaultCanvas;

  useLayoutEffect(() => {
    if (target == null) return;
    return attachMarqueeOverlay(target, {
      className,
      container,
      border,
      background,
      zIndex,
    });
  }, [background, border, className, container, target, zIndex]);

  return null;
}

export interface SelectionRingsProps extends SelectionRingsOptions {
  readonly selection: readonly PickingEntityRef[];
}

export function SelectionRings({
  selection,
  color,
  opacity,
  radiusScale,
  minRadius,
  yOffset,
  segments,
  renderOrder,
  geometry,
  material,
}: SelectionRingsProps) {
  const scene = useThree((state) => state.scene);
  const cache = useGaleonRendererCache();
  const controller = useRef<SelectionRingsController | null>(null);
  const options = useMemo<SelectionRingsOptions>(() => ({
    color,
    opacity,
    radiusScale,
    minRadius,
    yOffset,
    segments,
    renderOrder,
    geometry,
    material,
  }), [
    color,
    geometry,
    material,
    minRadius,
    opacity,
    radiusScale,
    renderOrder,
    segments,
    yOffset,
  ]);

  useLayoutEffect(() => {
    const rings = attachSelectionRings(scene, cache, options);
    controller.current = rings;
    rings.update(selection);
    return () => {
      controller.current = null;
      rings.dispose();
    };
  }, [cache, options, scene]);

  useLayoutEffect(() => {
    controller.current?.update(selection);
  }, [selection]);

  useFrame(() => {
    controller.current?.update(selection);
  });

  return null;
}
