// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! World-global mouse selection state.
//!
//! Input adapters (the WASM bridge for browsers, hand-written input layers for
//! native tools) apply pick and pick-rect events to the [`Selection`] resource.
//! Game systems read it through the [`Res<Selection>`](crate::Res) system param
//! to react to user input — issue movement orders, highlight units, etc.
//!
//! Modifier-key semantics follow the StarCraft / OpenRA consensus catalogued in
//! the `#214` discovery notes: `shift` = additive (toggle on click), `ctrl` =
//! subtractive, `alt` = intersect (rect-only). The TypeScript helper in
//! `@galeon/picking` reports modifiers per event; this module decides what
//! they mean for selection state.

use std::collections::HashSet;

use crate::entity::Entity;

/// Modifier-key bitmask for selection-modifying input events.
///
/// Encodes shift/ctrl/alt/meta in the low four bits so the WASM bridge can
/// pass a single `u32` across the JS boundary without serialising a struct.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PickModifiers(pub u32);

impl PickModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: u32 = 1 << 0;
    pub const CTRL: u32 = 1 << 1;
    pub const ALT: u32 = 1 << 2;
    pub const META: u32 = 1 << 3;

    /// Construct from boolean flags.
    pub fn from_bools(shift: bool, ctrl: bool, alt: bool, meta: bool) -> Self {
        let mut bits = 0;
        if shift {
            bits |= Self::SHIFT;
        }
        if ctrl {
            bits |= Self::CTRL;
        }
        if alt {
            bits |= Self::ALT;
        }
        if meta {
            bits |= Self::META;
        }
        Self(bits)
    }

    pub fn shift(self) -> bool {
        (self.0 & Self::SHIFT) != 0
    }
    pub fn ctrl(self) -> bool {
        (self.0 & Self::CTRL) != 0
    }
    pub fn alt(self) -> bool {
        (self.0 & Self::ALT) != 0
    }
    pub fn meta(self) -> bool {
        (self.0 & Self::META) != 0
    }
}

/// World-space hit point of the most recent click pick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// World-global selection state — current entity set plus last hit point.
///
/// Insert via [`World::insert_resource`](crate::World::insert_resource) before
/// any system that reads it. The TS helper emits both single-click and marquee
/// events; both flow into [`apply_pick`](Self::apply_pick) /
/// [`apply_pick_rect`](Self::apply_pick_rect).
#[derive(Default, Debug)]
pub struct Selection {
    /// Currently selected entities.
    pub entities: HashSet<Entity>,
    /// World-space point of the most recent successful click pick, if any.
    pub last_pick: Option<PickPoint>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a single-click pick.
    ///
    /// - **No modifier**: replace selection with `entity` (or clear if `None`).
    /// - **Shift**: toggle (add if absent, remove if present). Clicks on empty
    ///   space are no-ops to avoid accidental clears mid-shift.
    /// - **Ctrl**: subtract. Clicks on empty space are no-ops.
    /// - **Other modifier combinations**: same as no-modifier on a hit;
    ///   no-op on a miss.
    ///
    /// `last_pick` is updated whenever `point` is provided, regardless of
    /// modifier — game code may want the cursor position even on a clear.
    pub fn apply_pick(
        &mut self,
        entity: Option<Entity>,
        point: Option<PickPoint>,
        modifiers: PickModifiers,
    ) {
        if let Some(p) = point {
            self.last_pick = Some(p);
        }
        match entity {
            None => {
                if modifiers.0 == 0 {
                    self.entities.clear();
                }
            }
            Some(entity) => {
                if modifiers.shift() {
                    if !self.entities.insert(entity) {
                        self.entities.remove(&entity);
                    }
                } else if modifiers.ctrl() {
                    self.entities.remove(&entity);
                } else {
                    self.entities.clear();
                    self.entities.insert(entity);
                }
            }
        }
    }

    /// Apply a marquee pick.
    ///
    /// - **No modifier**: replace selection with `entities`.
    /// - **Shift**: add `entities` to selection.
    /// - **Ctrl**: remove `entities` from selection.
    /// - **Alt**: keep only entities present in both the current selection
    ///   and `entities` (set intersection).
    pub fn apply_pick_rect<I>(&mut self, entities: I, modifiers: PickModifiers)
    where
        I: IntoIterator<Item = Entity>,
    {
        if modifiers.shift() {
            self.entities.extend(entities);
        } else if modifiers.ctrl() {
            for e in entities {
                self.entities.remove(&e);
            }
        } else if modifiers.alt() {
            let new: HashSet<Entity> = entities.into_iter().collect();
            self.entities.retain(|e| new.contains(e));
        } else {
            self.entities.clear();
            self.entities.extend(entities);
        }
    }

    /// Whether `entity` is currently selected.
    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.contains(&entity)
    }

    /// Number of selected entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the selection is empty.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(index: u32, generation: u32) -> Entity {
        Entity::from_raw(index, generation)
    }

    #[test]
    fn modifiers_round_trip_through_bools() {
        let m = PickModifiers::from_bools(true, false, true, false);
        assert!(m.shift());
        assert!(!m.ctrl());
        assert!(m.alt());
        assert!(!m.meta());
    }

    #[test]
    fn click_replaces_selection_with_no_modifier() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.entities.insert(e(2, 0));
        sel.apply_pick(Some(e(3, 0)), None, PickModifiers::NONE);
        assert_eq!(sel.entities.len(), 1);
        assert!(sel.contains(e(3, 0)));
    }

    #[test]
    fn click_on_empty_space_clears_with_no_modifier() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.apply_pick(None, None, PickModifiers::NONE);
        assert!(sel.is_empty());
    }

    #[test]
    fn click_on_empty_space_with_modifier_is_noop() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.apply_pick(None, None, PickModifiers(PickModifiers::SHIFT));
        assert_eq!(sel.entities.len(), 1);
    }

    #[test]
    fn shift_click_toggles_membership() {
        let mut sel = Selection::new();
        sel.apply_pick(Some(e(1, 0)), None, PickModifiers(PickModifiers::SHIFT));
        sel.apply_pick(Some(e(2, 0)), None, PickModifiers(PickModifiers::SHIFT));
        assert_eq!(sel.entities.len(), 2);

        sel.apply_pick(Some(e(1, 0)), None, PickModifiers(PickModifiers::SHIFT));
        assert!(!sel.contains(e(1, 0)));
        assert!(sel.contains(e(2, 0)));
    }

    #[test]
    fn ctrl_click_subtracts() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.entities.insert(e(2, 0));
        sel.apply_pick(Some(e(1, 0)), None, PickModifiers(PickModifiers::CTRL));
        assert!(!sel.contains(e(1, 0)));
        assert!(sel.contains(e(2, 0)));
    }

    #[test]
    fn pick_records_last_world_point() {
        let mut sel = Selection::new();
        let p = PickPoint {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        sel.apply_pick(Some(e(1, 0)), Some(p), PickModifiers::NONE);
        assert_eq!(sel.last_pick, Some(p));
    }

    #[test]
    fn rect_no_modifier_replaces() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.apply_pick_rect([e(2, 0), e(3, 0)], PickModifiers::NONE);
        assert!(!sel.contains(e(1, 0)));
        assert!(sel.contains(e(2, 0)));
        assert!(sel.contains(e(3, 0)));
    }

    #[test]
    fn rect_shift_adds() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.apply_pick_rect([e(2, 0), e(3, 0)], PickModifiers(PickModifiers::SHIFT));
        assert_eq!(sel.entities.len(), 3);
    }

    #[test]
    fn rect_ctrl_subtracts() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.entities.insert(e(2, 0));
        sel.entities.insert(e(3, 0));
        sel.apply_pick_rect([e(2, 0), e(3, 0)], PickModifiers(PickModifiers::CTRL));
        assert_eq!(sel.entities.len(), 1);
        assert!(sel.contains(e(1, 0)));
    }

    #[test]
    fn rect_alt_intersects() {
        let mut sel = Selection::new();
        sel.entities.insert(e(1, 0));
        sel.entities.insert(e(2, 0));
        sel.entities.insert(e(3, 0));
        sel.apply_pick_rect(
            [e(2, 0), e(3, 0), e(4, 0)],
            PickModifiers(PickModifiers::ALT),
        );
        assert_eq!(sel.entities.len(), 2);
        assert!(sel.contains(e(2, 0)));
        assert!(sel.contains(e(3, 0)));
        assert!(!sel.contains(e(1, 0)));
    }
}
