# Virtual Time

Galeon's fixed-step loop receives raw elapsed time from the host. By default
that time flows straight into the tick accumulator. `VirtualTime` is an
optional resource that sits in front of the accumulator and transforms the
raw elapsed before any tick runs — enabling pause, speed scaling, and
max-delta clamping.

## Why VirtualTime?

**Death spirals.** If the host delivers a 5-second frame (tab was backgrounded,
debugger paused, machine slept), the accumulator would fire hundreds of ticks
to catch up, stalling the next rendered frame even longer. Max-delta clamping
breaks the spiral by capping how much virtual time can advance in a single
call.

**RTS speed controls.** Strategy games need fast-forward (2×, 4×) and
slow-motion replays. Setting `scale = 2.0` doubles the virtual time fed into
the accumulator, so the simulation ticks twice as often per real second.

**Editor pause.** The editor needs to freeze the simulation without stopping
the render loop (UI must stay live). Setting `paused = true` makes every tick
call produce zero virtual elapsed, so no simulation steps fire.

## Setup

```rust
use galeon_engine::VirtualTime;

engine.insert_resource(VirtualTime::new());
```

That is the only required step. Once the resource is present, `game_loop::tick`
reads it automatically.

### Backward compatibility

If `VirtualTime` is not inserted, `game_loop::tick` passes raw elapsed through
unchanged. Existing code needs no changes.

## Pausing

Via the `Engine` convenience API:

```rust
engine.pause();
engine.resume();
```

Both methods lazily insert `VirtualTime` if it is not already present.

From within a system:

```rust
fn pause_on_menu(world: &mut World) {
    world.resource_mut::<VirtualTime>().paused = true;
}
```

## Speed Scaling

```rust
engine.set_speed(2.0); // fast-forward (RTS)
engine.set_speed(0.5); // slow-motion replay
engine.set_speed(1.0); // normal
```

The scale is clamped to `[0.0, 8.0]` at tick time. Passing `0.0` is equivalent
to pausing without setting `paused = true`.

## Max-Delta Clamping

Raw elapsed is clamped to `max_delta` before the scale is applied. The default
is **0.25 seconds**.

```
virtual_elapsed = min(raw, max_delta) × scale
```

At 10 Hz with `max_delta = 0.25` and a 5-second host frame, the accumulator
receives 0.25 s and fires 2 ticks instead of 50. The remaining real time is
simply discarded — the simulation skips ahead by at most one `max_delta`
window per call.

To customize:

```rust
let mut vt = VirtualTime::new();
vt.max_delta = 0.1; // tighter cap for network-sensitive games
engine.insert_resource(vt);
```

## Reading VirtualTime in Systems

```rust
fn hud_system(world: &mut World) {
    let vt = world.resource::<VirtualTime>();
    let elapsed = vt.elapsed; // total virtual seconds since engine start
    let scale   = vt.scale;
    let paused  = vt.paused;
}
```

`elapsed` accumulates virtual seconds, not real seconds. At 2× speed, it
advances twice as fast as wall-clock time.

## WASM / JavaScript

`WasmEngine` exposes the same controls to the JS host:

```js
wasmEngine.pause();
wasmEngine.resume();
wasmEngine.set_speed(2.0);
const paused = wasmEngine.is_paused();
```

This is the primary control surface for browser games and the web-target
editor.

## Interaction with FixedTimestep

`VirtualTime` and `FixedTimestep` are separate concerns:

- `VirtualTime` transforms the **input** (raw elapsed → virtual elapsed).
- `FixedTimestep` manages the **accumulator** (virtual elapsed → discrete ticks).

Pausing virtual time stops ticks because zero virtual elapsed never advances
the accumulator past the step threshold. Speed scaling produces more ticks per
real second because more virtual time is fed in. The step size itself does not
change.
