# Effect Boundary Evaluation

Issue: #207
Base: stacked on #205 / PR #206 adapter split
Date: 2026-04-24

## Decision

Defer adopting Effect for Galeon's current TypeScript render packages.

Effect is a credible fit for non-hot TypeScript boundaries, but the first
candidate in `@galeon/render-core` shows that Galeon's current packet validation
is mostly cross-field typed-array invariants. Schema can describe object shape,
but the important checks still need custom validation. Adding Effect here would
increase dependency and style surface without removing enough code or risk.

## Candidate Selection

The evaluated candidate was `@galeon/render-core` packet validation:

- It is adapter boundary code, not Rust-owned engine behavior.
- It validates external WASM-facing data before host adapters consume it.
- It is package-local and removable.
- It avoids `RendererCache.applyFrame` and R3F hot synchronization loops.

This was the right place to test adoption pressure, but not the right place to
ship the dependency now.

## Sketch

This is the shape an Effect-backed boundary would likely take if revisited for a
non-hot validation surface:

```typescript
// packages/render-core/src/effect-contract.ts
// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import { Data, Effect, Schema } from "effect";
import {
  RENDER_CONTRACT_VERSION,
  TRANSFORM_STRIDE,
  assertFramePacketContract,
  type FramePacketView,
} from "./index.js";

class FramePacketDecodeError extends Data.TaggedError("FramePacketDecodeError")<{
  readonly message: string;
}> {}

const NonNegativeInteger = Schema.Number.pipe(
  Schema.filter((value) => Number.isInteger(value) && value >= 0),
);

const FramePacketBoundary = Schema.Struct({
  contract_version: Schema.optional(Schema.Number),
  entity_count: NonNegativeInteger,
  custom_channel_count: NonNegativeInteger,
});

export const decodeFramePacketBoundary = (
  input: unknown,
): Effect.Effect<FramePacketView, FramePacketDecodeError> =>
  Schema.decodeUnknown(FramePacketBoundary)(input).pipe(
    Effect.mapError(
      (error) =>
        new FramePacketDecodeError({
          message: String(error),
        }),
    ),
    Effect.flatMap(() =>
      Effect.try({
        try: () => {
          const packet = input as FramePacketView;
          assertFramePacketContract(packet);
          if (
            packet.contract_version != null &&
            packet.contract_version !== RENDER_CONTRACT_VERSION
          ) {
            throw new Error("unsupported render contract version");
          }
          if (packet.transforms.length !== packet.entity_count * TRANSFORM_STRIDE) {
            throw new Error("invalid transform table length");
          }
          return packet;
        },
        catch: (error) =>
          new FramePacketDecodeError({
            message: error instanceof Error ? error.message : String(error),
          }),
      }),
    ),
  );
```

The sketch makes the tradeoff visible: the useful checks remain in
`assertFramePacketContract`, while Effect mainly wraps failure typing. That is
valuable for async/tooling boundaries, but thin for this render contract.

## Recommended Adoption Boundary

Revisit Effect when Galeon has one of these package-local surfaces:

- editor/shell startup resource lifecycle for WASM module initialization;
- manifest or debug snapshot JSON validation;
- CLI/dev tooling config with typed startup failures;
- tests that benefit from swappable setup layers.

Keep Effect out of:

- `RendererCache.applyFrame`;
- R3F frame-loop synchronization;
- packet transform/object iteration;
- Rust-owned engine behavior;
- public render contract types.

## Impact

Bundle/API impact: no dependency added now. A future adoption should be
package-local and should not leak Effect types through `@galeon/render-core`
exports.

Testing impact: existing TypeScript checks remain the verification gate. A
future Effect prototype should include package-local tests that compare decoded
failures against current `FramePacketContractError` behavior.

Maintainability impact: defer avoids forcing a new programming model into the
smallest render package. The next spike should target a richer boundary where
Schema, config, typed errors, and resource lifecycle all participate.
