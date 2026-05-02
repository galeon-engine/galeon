// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { useFrame, useThree } from "@react-three/fiber";
import {
  attachMarqueeRenderer,
  attachSelectionRings,
  type MarqueeRect,
  type MarqueeRendererController,
  type MarqueeRendererOptions,
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

export interface MarqueeRendererProps extends MarqueeRendererOptions {
  readonly rect: MarqueeRect | null;
}

export function MarqueeRenderer({
  rect,
  color,
  opacity,
  renderOrder,
  zOffset,
  material,
}: MarqueeRendererProps) {
  const camera = useThree((state) => state.camera);
  const controller = useRef<MarqueeRendererController | null>(null);
  const options = useMemo<MarqueeRendererOptions>(() => ({
    color,
    opacity,
    renderOrder,
    zOffset,
    material,
  }), [color, material, opacity, renderOrder, zOffset]);

  useLayoutEffect(() => {
    const marquee = attachMarqueeRenderer(camera, options);
    controller.current = marquee;
    marquee.update(rect);
    return () => {
      controller.current = null;
      marquee.dispose();
    };
  }, [camera, options]);

  useLayoutEffect(() => {
    controller.current?.update(rect);
  }, [rect]);

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
