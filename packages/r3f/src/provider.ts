// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { useFrame, useThree } from "@react-three/fiber";
import {
  assertFramePacketContract,
  type FramePacketView,
} from "@galeon/render-core";
import { RendererCache } from "@galeon/three";
import {
  Fragment,
  createContext,
  createElement,
  useCallback,
  useContext,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  type ReactNode,
} from "react";
import { GaleonEntityStore, type GaleonEntityRef } from "./entity-store.js";

interface GaleonR3FContextValue {
  cache: RendererCache;
  store: GaleonEntityStore;
  revision: number;
}

const GaleonR3FContext = createContext<GaleonR3FContextValue | null>(null);

export interface GaleonR3FProviderProps {
  /**
   * Externally supplied frame, useful for tests or apps that own ticking
   * outside the R3F render loop.
   */
  frame?: FramePacketView | null | undefined;
  /**
   * Engine-like source for R3F-driven scenes. `tick` is optional so callers can
   * drive simulation elsewhere while the provider still consumes snapshots.
   */
  engine?: {
    tick?: (deltaSeconds: number) => unknown;
    extract_frame: () => FramePacketView;
  };
  children?: ReactNode;
  /** Call `engine.tick(delta)` from `useFrame`. Defaults to true. */
  driveTicks?: boolean;
  /** Require `contract_version` on incoming packets. Defaults to strict mode. */
  requireContractVersion?: boolean;
}

export function GaleonR3FProvider({
  frame,
  engine,
  children,
  driveTicks = true,
  requireContractVersion = true,
}: GaleonR3FProviderProps) {
  const scene = useThree((state) => state.scene);
  const cache = useMemo(() => new RendererCache(scene), [scene]);
  const storeRef = useRef<GaleonEntityStore>(new GaleonEntityStore());
  const store = storeRef.current;
  const [revision, bumpRevision] = useReducer(
    (value: number) => value + 1,
    0,
  );

  useLayoutEffect(
    () => () => {
      cache.dispose();
      store.clear();
    },
    [cache, store],
  );

  const applyGaleonFrame = useCallback((nextFrame: FramePacketView | null | undefined) => {
    if (nextFrame == null) {
      return;
    }

    assertFramePacketContract(nextFrame, {
      allowMissingContractVersion: !requireContractVersion,
    });
    cache.applyFrame(nextFrame);
    if (!cache.needsRender) {
      return;
    }

    if (store.sync(nextFrame, cache)) {
      bumpRevision();
    }
  }, [cache, requireContractVersion, store]);

  useLayoutEffect(() => {
    applyGaleonFrame(frame);
  }, [applyGaleonFrame, frame]);

  useFrame((_, delta) => {
    if (engine === undefined) {
      return;
    }

    if (driveTicks) {
      engine.tick?.(delta);
    }
    applyGaleonFrame(engine.extract_frame());
  });

  const contextValue = useMemo(
    () => ({ cache, store, revision }),
    [cache, store, revision],
  );

  return createElement(GaleonR3FContext.Provider, { value: contextValue }, children);
}

function useGaleonR3FContext(): GaleonR3FContextValue {
  const value = useContext(GaleonR3FContext);
  if (value === null) {
    throw new Error(
      "Galeon R3F hooks must be used under <GaleonR3FProvider />",
    );
  }
  return value;
}

export function useGaleonRendererCache(): RendererCache {
  return useGaleonR3FContext().cache;
}

export function useGaleonFrameRevision(): number {
  return useGaleonR3FContext().revision;
}

export function useGaleonEntities(): readonly GaleonEntityRef[] {
  const { store, revision } = useGaleonR3FContext();
  void revision;
  return store.entities();
}

export function useGaleonEntity(
  entityId: number,
  generation: number,
): GaleonEntityRef | undefined {
  const { store, revision } = useGaleonR3FContext();
  void revision;
  return store.get(entityId, generation);
}

export interface GaleonEntitiesProps {
  children: (entity: GaleonEntityRef) => ReactNode;
}

export function GaleonEntities({ children }: GaleonEntitiesProps) {
  const entities = useGaleonEntities();
  return createElement(
    Fragment,
    null,
    entities.map((entity) =>
      createElement(Fragment, { key: entity.key }, children(entity)),
    ),
  );
}
