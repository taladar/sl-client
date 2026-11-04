//! What a plain **left click** on a prim does: its click action, a touch, or
//! nothing (`viewer-left-click-click-actions`).
//!
//! A prim's click action is the viewer's to carry out. Neither grid withholds a
//! touch from a prim set to *sit* or *disabled* — a script in one is handed its
//! touch events all the same when a viewer sends the grab
//! (`book/src/gridspec/building.md` § Touch and grab) — so the decision lives
//! here, in the viewer, exactly where the reference keeps it
//! (`LLToolPie::handleLeftClickPick`):
//!
//! 1. **The click action**, for a prim in the world (never an attachment) whose
//!    own action or whose root's is something other than *touch* or
//!    *disabled* (`useClickAction`). The prim's own action wins when it has
//!    one, the root's otherwise. *Sit* sits — unless the avatar already sits,
//!    or the user blocked click-sitting (Firestorm's `FSBlockClickSit`), when
//!    it falls through to *pay* as the reference's `switch` does; *pay* pays a
//!    prim whose script takes money, and otherwise touches; *open* opens a root
//!    whose contents this agent may see, and otherwise does nothing; *buy*,
//!    *play*, *open media* and *zoom* each do their own thing; a prim whose own
//!    *disabled* masks its root's action does nothing at all.
//! 2. **The touch**, for a prim that is physical, or whose script (on it or its
//!    root) takes touches and whose own click action is not *disabled*.
//! 3. **Nothing** for any other prim: no grab goes out, so a click on a plain
//!    build is not a touch in a packet trace either.
//!
//! [`final_click_action`] is the reference's `final_click_action`, the action a
//! *hover* or a double-click reads, which differs from the left click's in one
//! case: a child with no action under a *disabled* root is touch there.

use bevy::prelude::*;
use sl_client_bevy::{ClickAction, SurfaceInfo};

use crate::object_flags::{
    FLAGS_HANDLE_TOUCH, FLAGS_INVENTORY_EMPTY, FLAGS_OBJECT_MODIFY, FLAGS_OBJECT_YOU_OWNER,
    FLAGS_TAKES_MONEY, FLAGS_USE_PHYSICS,
};
use crate::object_graph::ObjectPickSummary;

/// The settings key of Firestorm's **block left-click sitting** option
/// (`FSBlockClickSit`, off by default): a left click on a prim whose click
/// action is *sit* no longer seats the avatar, and falls through to whatever
/// else the prim takes — a touch, where a script listens for one.
pub const SETTING_BLOCK_CLICK_SIT: &str = "FSBlockClickSit";

/// The click action a click on a prim carries out — the reference's
/// `final_click_action`: none (touch) for an attachment, the picked prim's own
/// when it has one or its root's is *disabled* (which masks the root's for its
/// children), the root's otherwise.
#[must_use]
pub const fn final_click_action(picked: u8, root: u8, attachment: bool) -> ClickAction {
    if attachment {
        ClickAction::Touch
    } else if picked != 0 || root == ClickAction::Disabled.to_code() {
        ClickAction::from_code(picked)
    } else {
        ClickAction::from_code(root)
    }
}

/// A click action the left click hands to the feature behind it, rather than
/// touching the prim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickedAction {
    /// Sit on the picked prim.
    Sit,
    /// Pay the picked prim.
    Pay,
    /// Buy the object (its root).
    Buy,
    /// Open the object's (its root's) contents.
    Open,
    /// Play or pause the parcel's media.
    Play,
    /// Open the parcel's media.
    OpenMedia,
    /// Bring the camera to the picked prim.
    Zoom,
}

/// What one left click on a prim comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftClick {
    /// Carry out a click action instead of touching.
    Act(ClickedAction),
    /// Touch the prim: the `ObjectGrab` of the grab tool.
    Touch,
    /// Nothing: the prim takes no click.
    Nothing,
}

/// The agent's side of a left click: what the prim's flags and actions do not
/// say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClickerState {
    /// Whether the agent's avatar is sitting, on an object or on the ground.
    pub sitting: bool,
    /// Whether the user blocked click-sitting ([`SETTING_BLOCK_CLICK_SIT`]).
    pub block_click_sit: bool,
}

/// What a plain left click on the prim `summary` describes does, for an agent
/// in `clicker`'s state. See the [module documentation](self).
#[must_use]
pub fn left_click(summary: &ObjectPickSummary, clicker: ClickerState) -> LeftClick {
    let picked = ClickAction::from_code(summary.picked_click_action);
    let root = ClickAction::from_code(summary.root_click_action);
    let acts = |action: ClickAction| !matches!(action, ClickAction::Touch | ClickAction::Disabled);
    // `useClickAction`: a prim in the world whose own action or root's does
    // something; touch and disabled are not actions it handles.
    if !summary.attachment && (acts(picked) || acts(root)) {
        let takes_money = summary.flags & FLAGS_TAKES_MONEY != 0;
        let action = if picked == ClickAction::Touch {
            root
        } else {
            picked
        };
        match action {
            ClickAction::Sit if !clicker.sitting && !clicker.block_click_sit => {
                return LeftClick::Act(ClickedAction::Sit);
            }
            // A sit the agent cannot take falls into the pay case, as the
            // reference's `switch` does, and on to the touch below.
            ClickAction::Sit | ClickAction::Pay if takes_money => {
                return LeftClick::Act(ClickedAction::Pay);
            }
            ClickAction::Buy => return LeftClick::Act(ClickedAction::Buy),
            ClickAction::Open => {
                // `allowOpen`, on the root: contents to show, and an agent
                // who owns or may modify it. Neither way is it a touch.
                let root_flags = summary.root_flags;
                let may_open = root_flags & FLAGS_INVENTORY_EMPTY == 0
                    && root_flags & (FLAGS_OBJECT_YOU_OWNER | FLAGS_OBJECT_MODIFY) != 0;
                return if may_open {
                    LeftClick::Act(ClickedAction::Open)
                } else {
                    LeftClick::Nothing
                };
            }
            ClickAction::Play => return LeftClick::Act(ClickedAction::Play),
            ClickAction::OpenMedia => return LeftClick::Act(ClickedAction::OpenMedia),
            ClickAction::Zoom => return LeftClick::Act(ClickedAction::Zoom),
            ClickAction::Disabled => return LeftClick::Nothing,
            // A sit or pay that cannot be taken, `Ignore`, and any action a
            // later protocol adds: the reference's `default`, on to the touch.
            _other => {}
        }
    }
    let touchable = picked != ClickAction::Disabled && summary.flags & FLAGS_HANDLE_TOUCH != 0;
    if touchable || summary.flags & FLAGS_USE_PHYSICS != 0 {
        LeftClick::Touch
    } else {
        LeftClick::Nothing
    }
}

/// A left click's click action, for the feature that carries it out: the
/// left-click pick (`sl_viewer_world_view::hud_pick`) writes it in place of a
/// touch, and the object menu's handler — which holds the same sit, open and
/// pay machinery its pie slices use — answers it.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct ClickActionRequest {
    /// The action to carry out.
    pub action: ClickedAction,
    /// The prim the click picked, its root and their flags.
    pub summary: ObjectPickSummary,
    /// Where on the prim the click landed, in the region's frame.
    pub surface: SurfaceInfo,
}

#[cfg(test)]
mod tests {
    use super::{ClickedAction, ClickerState, LeftClick, final_click_action, left_click};
    use crate::object_flags::{
        FLAGS_HANDLE_TOUCH, FLAGS_INVENTORY_EMPTY, FLAGS_OBJECT_MODIFY, FLAGS_OBJECT_YOU_OWNER,
        FLAGS_TAKES_MONEY, FLAGS_USE_PHYSICS,
    };
    use crate::object_graph::ObjectPickSummary;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        CircuitId, ClickAction, ObjectKey, RegionLocalObjectId, ScopedObjectId, Uuid,
    };

    /// A standing agent that has not blocked click-sitting.
    const STANDING: ClickerState = ClickerState {
        sitting: false,
        block_click_sit: false,
    };

    /// A prim in the world, its own click action `picked` under a root with
    /// `root`, carrying the (picked ∪ root) `flags` and the root's own
    /// `root_flags`.
    fn prim(
        picked: ClickAction,
        root: ClickAction,
        flags: u32,
        root_flags: u32,
    ) -> ObjectPickSummary {
        let scoped = ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId::new(7));
        ObjectPickSummary {
            picked_scoped: scoped,
            picked_full: ObjectKey::from(Uuid::from_u128(7)),
            root_scoped: scoped,
            root_full: ObjectKey::from(Uuid::from_u128(7)),
            flags,
            root_flags,
            picked_click_action: picked.to_code(),
            root_click_action: root.to_code(),
            attachment: false,
            wearer: None,
        }
    }

    /// The same prim, worn.
    fn worn(mut summary: ObjectPickSummary) -> ObjectPickSummary {
        summary.attachment = true;
        summary
    }

    /// Each action that has its own behaviour is carried out instead of a
    /// touch — even on a prim whose script takes touches.
    #[test]
    fn an_action_replaces_the_touch() {
        for (action, expected) in [
            (ClickAction::Sit, ClickedAction::Sit),
            (ClickAction::Buy, ClickedAction::Buy),
            (ClickAction::Play, ClickedAction::Play),
            (ClickAction::OpenMedia, ClickedAction::OpenMedia),
            (ClickAction::Zoom, ClickedAction::Zoom),
        ] {
            assert_eq!(
                left_click(
                    &prim(action, ClickAction::Touch, FLAGS_HANDLE_TOUCH, 0),
                    STANDING
                ),
                LeftClick::Act(expected),
                "{action:?}"
            );
        }
    }

    /// *Disabled* takes the click away from a script: no action and no touch,
    /// even from a script that listens for one. A **physical** prim is still
    /// grabbed, as in the reference — its grab rule reads physics, not the
    /// click action.
    #[test]
    fn disabled_does_nothing() {
        let summary = prim(
            ClickAction::Disabled,
            ClickAction::Touch,
            FLAGS_HANDLE_TOUCH,
            0,
        );
        assert_eq!(left_click(&summary, STANDING), LeftClick::Nothing);
        let summary = prim(
            ClickAction::Disabled,
            ClickAction::Touch,
            FLAGS_HANDLE_TOUCH | FLAGS_USE_PHYSICS,
            0,
        );
        assert_eq!(left_click(&summary, STANDING), LeftClick::Touch);
        // A disabled child masks its root's action too.
        let summary = prim(ClickAction::Disabled, ClickAction::Sit, 0, 0);
        assert_eq!(left_click(&summary, STANDING), LeftClick::Nothing);
    }

    /// A child with no action of its own takes its root's.
    #[test]
    fn a_child_takes_its_roots_action() {
        let summary = prim(ClickAction::Touch, ClickAction::Zoom, 0, 0);
        assert_eq!(
            left_click(&summary, STANDING),
            LeftClick::Act(ClickedAction::Zoom)
        );
    }

    /// **A sit the agent cannot take** — already sitting, or click-sitting
    /// blocked — is not a sit: it pays a prim that takes money, touches one
    /// whose script takes touches, and otherwise does nothing.
    #[test]
    fn a_sit_that_cannot_be_taken_falls_through() {
        for clicker in [
            ClickerState {
                sitting: true,
                block_click_sit: false,
            },
            ClickerState {
                sitting: false,
                block_click_sit: true,
            },
        ] {
            let sit = |flags| {
                left_click(
                    &prim(ClickAction::Sit, ClickAction::Touch, flags, 0),
                    clicker,
                )
            };
            assert_eq!(sit(0), LeftClick::Nothing, "{clicker:?}");
            assert_eq!(sit(FLAGS_HANDLE_TOUCH), LeftClick::Touch, "{clicker:?}");
            assert_eq!(
                sit(FLAGS_TAKES_MONEY),
                LeftClick::Act(ClickedAction::Pay),
                "{clicker:?}"
            );
        }
    }

    /// *Pay* pays only a prim whose script takes money, and otherwise touches
    /// as an ordinary prim would.
    #[test]
    fn pay_needs_a_money_script() {
        let pay = |flags| {
            left_click(
                &prim(ClickAction::Pay, ClickAction::Touch, flags, 0),
                STANDING,
            )
        };
        assert_eq!(pay(FLAGS_TAKES_MONEY), LeftClick::Act(ClickedAction::Pay));
        assert_eq!(pay(FLAGS_HANDLE_TOUCH), LeftClick::Touch);
        assert_eq!(pay(0), LeftClick::Nothing);
    }

    /// *Open* opens a root with contents its owner — or anyone who may modify
    /// it — can see, and is never a touch.
    #[test]
    fn open_needs_contents_the_agent_may_see() {
        let open = |root_flags| {
            left_click(
                &prim(
                    ClickAction::Open,
                    ClickAction::Touch,
                    FLAGS_HANDLE_TOUCH,
                    root_flags,
                ),
                STANDING,
            )
        };
        assert_eq!(
            open(FLAGS_OBJECT_YOU_OWNER),
            LeftClick::Act(ClickedAction::Open)
        );
        assert_eq!(
            open(FLAGS_OBJECT_MODIFY),
            LeftClick::Act(ClickedAction::Open)
        );
        assert_eq!(open(0), LeftClick::Nothing, "not the agent's to open");
        assert_eq!(
            open(FLAGS_OBJECT_YOU_OWNER | FLAGS_INVENTORY_EMPTY),
            LeftClick::Nothing,
            "nothing to open"
        );
    }

    /// **The touch goes only to a prim that takes one**: a script listening
    /// for touches, or physics. A plain prim gets nothing on the wire.
    #[test]
    fn only_a_prim_that_takes_a_touch_is_touched() {
        let touch = |flags| {
            left_click(
                &prim(ClickAction::Touch, ClickAction::Touch, flags, 0),
                STANDING,
            )
        };
        assert_eq!(touch(FLAGS_HANDLE_TOUCH), LeftClick::Touch);
        assert_eq!(touch(FLAGS_USE_PHYSICS), LeftClick::Touch);
        assert_eq!(touch(0), LeftClick::Nothing);
        // `Ignore` is an action the left click has no case for: the touch.
        let ignore = prim(
            ClickAction::Ignore,
            ClickAction::Touch,
            FLAGS_HANDLE_TOUCH,
            0,
        );
        assert_eq!(left_click(&ignore, STANDING), LeftClick::Touch);
    }

    /// **A worn prim never carries out a click action** — a sit attachment is
    /// touched like any other, where it takes one.
    #[test]
    fn an_attachment_only_touches() {
        let summary = worn(prim(
            ClickAction::Sit,
            ClickAction::Sit,
            FLAGS_HANDLE_TOUCH,
            0,
        ));
        assert_eq!(left_click(&summary, STANDING), LeftClick::Touch);
        let summary = worn(prim(ClickAction::Zoom, ClickAction::Touch, 0, 0));
        assert_eq!(left_click(&summary, STANDING), LeftClick::Nothing);
    }

    /// `final_click_action`: none for an attachment; the prim's own when it has
    /// one or its root disables its own; the root's otherwise.
    #[test]
    fn the_final_action_follows_the_reference() {
        let code = ClickAction::to_code;
        assert_eq!(
            final_click_action(code(ClickAction::Touch), code(ClickAction::Sit), false),
            ClickAction::Sit
        );
        assert_eq!(
            final_click_action(code(ClickAction::Pay), code(ClickAction::Sit), false),
            ClickAction::Pay
        );
        assert_eq!(
            final_click_action(code(ClickAction::Touch), code(ClickAction::Disabled), false),
            ClickAction::Touch
        );
        assert_eq!(
            final_click_action(code(ClickAction::Sit), code(ClickAction::Sit), true),
            ClickAction::Touch
        );
    }
}
