//! The viewer's **RLVa control surface**: the windows a person opens to see
//! what an object has done to their viewer, and to do it to themselves on
//! purpose.
//!
//! The `sl-rlv` crate is the engine — it decodes the `@`-command language,
//! holds what the held commands mean, and answers what a restriction allows.
//! [`sl_viewer_world_api::rlv`] is where the viewer keeps one of those state
//! machines and the settings that steer it. This crate is the four windows the
//! reference's RLVa menu opens onto that state:
//!
//! - [`rlv_console`] — type RLV commands at your own viewer and watch what
//!   happens to each one. The debugging and authoring tool, and the only place
//!   in the viewer where the agent talks `@` to itself.
//! - [`rlv_behaviours`] — the live list of what is in force: the restrictions
//!   grouped by the object holding each, the exceptions poked in them, and the
//!   modifier slots with the values objects have written.
//! - [`rlv_locks`] — the lock model's four registries, which is the question a
//!   yes/no restriction cannot answer: not "is detaching blocked" but "may
//!   *this* come off".
//! - [`rlv_strings`] — the canned texts RLVa emits, and the user's rewrites of
//!   them.
//!
//! It is also where the **command intake** lives ([`intake`]): the owner-say
//! gate a worn collar actually speaks through, the one seam every answer is
//! chatted back by, and the pass that lifts a vanished object's restrictions.
//! That is not a window, but it is the same feature and the same state — and
//! keeping it here rather than one tier down keeps the world-API crate what it
//! is, a crate of types with no schedules of its own.
//!
//! # Why the windows are here and the state is not
//!
//! The floaters *draw* the RLV state, but the chat bar, the session command
//! path and every wear path have to **ask** it — and none of those may depend
//! on the crate that draws windows. So the state lives one tier down, in
//! `sl-viewer-world-api`, and this crate only reads it. That is the same split
//! the selection, the mute list and the presence modes already follow.
//!
//! # Every window is gated on the master switch
//!
//! `RestrainedLove` is off until the user turns it on, and while it is off the
//! four windows are unreachable: the RLVa menu itself is the only entry point,
//! and it hides. That is the reference's arrangement and it is the right one —
//! a viewer that can be restrained by a worn object should never become one by
//! accident.

#![expect(
    clippy::module_name_repetitions,
    reason = "each window's module is named for the RLVa window it draws, and its \
              floater-spec constructor must be `<module>_floater_spec` for the \
              viewer's registry guard to find it — so the repetition is the \
              protocol, not an accident of naming"
)]

pub mod intake;
pub mod rlv_behaviours;
pub mod rlv_console;
pub mod rlv_locks;
pub mod rlv_strings;

use bevy::prelude::*;

/// The shared palette and geometry the four RLVa windows are drawn with — the
/// values the sibling list floaters (the block list, the radar, the asset
/// blacklist) already use, kept in one place so the family reads as one.
pub(crate) mod style {
    use bevy::prelude::Color;

    use sl_viewer_ui_core::skin_palette::SkinPalette;

    /// Header / cell font size, logical px.
    pub(crate) const FONT_SIZE: f32 = 13.0;

    /// Table row height, logical px.
    pub(crate) const ROW_HEIGHT: f32 = 20.0;

    /// The default cell / label colour.
    pub(crate) const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

    /// The dimmed header / secondary colour.
    pub(crate) const DIM_LABEL_COLOR: Color = SkinPalette::FALLBACK.text_muted;

    /// An action button's background.
    pub(crate) const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);
}

/// The fixed restriction set the RLVa windows' gallery specimens draw — one
/// state, so the Restrictions and Locks specimens show the same two objects'
/// commands from their two angles, as the live windows would.
pub(crate) mod specimen {
    use sl_client_bevy::Uuid;
    use sl_rlv::{RlvAttachmentPoint, RlvObjectAttachment, RlvState, parse_chat_line};

    /// A sample collar: worn (so its bare `@detach` is an attachment lock),
    /// restricting IMs with one exception, and locking a point, a layer and a
    /// folder.
    const COLLAR: Uuid = Uuid::from_u128(0x3f2a_91c4_5d7e_4b10_8a6f_2c94_e1d0_7b35);

    /// A sample pair of cuffs, not reported as worn — so its restrictions are
    /// named by key alone, the way an object out of range is.
    const CUFFS: Uuid = Uuid::from_u128(0x8c41_07e2_b93a_4f5d_9e12_6a7b_c3d8_0f64);

    /// The resident the collar's IM exception lets through.
    const FRIEND: Uuid = Uuid::from_u128(0x5d90_2b7e_1c4a_4e83_b6f1_09a2_d7c5_e318);

    /// The sample state: both objects' commands applied, the collar placed.
    ///
    /// A line that fails to parse is skipped rather than reported: the lines are
    /// fixed here, and `the_sample_state_holds_every_line` pins that each one
    /// parses and lands.
    pub(crate) fn sample_state() -> RlvState {
        let mut state = RlvState::new();
        for (object, line) in sample_lines() {
            for command in parse_chat_line(&line).into_iter().flatten().flatten() {
                state.apply(object, &command);
            }
        }
        if let Some(point) = RlvAttachmentPoint::from_name("neck") {
            state.set_object_attachment(COLLAR, Some(RlvObjectAttachment::new(COLLAR, point)));
        }
        state
    }

    /// The chat lines the two sample objects say, in order.
    fn sample_lines() -> [(Uuid, String); 2] {
        [
            (
                COLLAR,
                format!(
                    "@detach=n,sendim=n,sendim:{FRIEND}=add,tplure=n,fartouch:1.5=n,\
                     remattach:chest=n,remoutfit:gloves=n,detachallthis:Outfits/Locked=n"
                ),
            ),
            (
                CUFFS,
                "@fly=n,recvchat=n,sittp:2.5=n,addattach=n".to_owned(),
            ),
        ]
    }

    #[cfg(test)]
    mod tests {
        use super::{sample_lines, sample_state};
        use pretty_assertions::assert_eq;
        use sl_rlv::parse_chat_line;

        /// Every sample line parses cleanly, and every command in it is held —
        /// so a specimen never silently shows fewer rows than its lines say.
        #[test]
        fn the_sample_state_holds_every_line() {
            let mut commands = 0_usize;
            for (_, line) in sample_lines() {
                let parsed = parse_chat_line(&line).unwrap_or_default();
                assert!(!parsed.is_empty(), "{line} is not an RLV line");
                for command in parsed {
                    assert!(command.is_ok(), "{line}: {command:?}");
                    commands = commands.saturating_add(1);
                }
            }
            let state = sample_state();
            let held: usize = state
                .restricting_objects()
                .map(|object| state.restrictions_of(object).len())
                .sum();
            assert_eq!(held, commands);
        }
    }
}

/// Every RLVa window at once, for a host that wants the whole family.
///
/// The viewer adds the four plugins individually alongside its other floaters;
/// this exists so a smaller host (the gallery, a test app) can take the set in
/// one line without having to know how many there are.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvUiPlugins;

impl Plugin for RlvUiPlugins {
    fn build(&self, app: &mut App) {
        // The one state machine every RLV surface consults. Initialised here
        // rather than in `sl-viewer-world-api` because that crate holds types,
        // not schedules — the same reason the selection and the mute list are
        // initialised by the features that fill them.
        app.init_resource::<sl_viewer_world_api::rlv::RlvSession>();
        // The seam the `@setenv_*` family writes through and the scene reads.
        // Initialised here for the same reason: the scene's half only runs in a
        // viewer with a world, but a console typing `@setenv_ambient` must have
        // somewhere to write even in one without.
        app.init_resource::<sl_viewer_world_api::rlv::RlvEnvironmentSlot>();
        app.add_plugins((
            rlv_console::RlvConsolePlugin,
            rlv_behaviours::RlvBehavioursPlugin,
            rlv_locks::RlvLocksPlugin,
            rlv_strings::RlvStringsPlugin,
        ));
    }
}
