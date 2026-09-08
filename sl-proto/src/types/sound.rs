//! The built-in **UI sound** assets a viewer plays for its own events — the
//! typing chirp, the money chime, the teleport whoosh, the snapshot shutter.
//!
//! These are the reference viewer's `UISnd*` settings defaults
//! (`indra/newview/app_settings/settings.xml`). Every one of them is an
//! ordinary **library asset**: the reference viewer ships no `.wav`/`.ogg`
//! anywhere in its tree — its `static_assets` / `fs_static_assets` folders hold
//! only animations, wearables and gestures — so a viewer fetches each over
//! `ViewerAsset` like any other sound, and a grid that serves none of them
//! leaves an arrival's every UI sound silent and its log full of failed
//! fetches.
//!
//! They live here, beside the built-in
//! [textures](crate::BUILTIN_ENVIRONMENT_TEXTURES), for the same reason those
//! do: the ids are shared between the viewer that asks for them and the grid
//! fixture that answers them, and neither should own the other's copy.

use uuid::Uuid;

/// A generic button click (`UISndClick`, and `UISndClickRelease`, which the
/// reference defaults to the same asset).
pub const UI_SOUND_CLICK: Uuid = Uuid::from_u128(0x4c8c_3c77_de8d_bde2_b9b8_3263_5e0f_d4a6);

/// The typing chirp played while entering local chat (`UISndTyping`).
pub const UI_SOUND_TYPING: Uuid = Uuid::from_u128(0x5e19_1c7b_8996_9ced_a177_b2ac_32bf_ea06);

/// A generic alert / notification (`UISndAlert`, and the friend on/offline
/// chimes, which default to it).
pub const UI_SOUND_ALERT: Uuid = Uuid::from_u128(0xed12_4764_705d_d497_167a_182c_d9fa_2e6c);

/// An invalid operation / rejected input (`UISndInvalidOp`).
pub const UI_SOUND_INVALID_OP: Uuid = Uuid::from_u128(0x4174_f859_0d3d_c517_c424_7292_3dc2_1f65);

/// Money received — a credit (`UISndMoneyChangeUp`).
pub const UI_SOUND_MONEY_UP: Uuid = Uuid::from_u128(0x77a0_18af_098e_c037_51a6_178f_0587_7c6f);

/// Money paid — a debit (`UISndMoneyChangeDown`).
pub const UI_SOUND_MONEY_DOWN: Uuid = Uuid::from_u128(0x1049_74e3_dfda_428b_99ee_b0d4_e748_d3a3);

/// A teleport starting (`UISndTeleportOut`).
pub const UI_SOUND_TELEPORT_OUT: Uuid = Uuid::from_u128(0xd7a9_a565_a013_2a69_797d_5332_baa1_a947);

/// The snapshot shutter (`UISndSnapshot`).
pub const UI_SOUND_SNAPSHOT: Uuid = Uuid::from_u128(0x3d09_f582_3851_c0e0_f5ba_277a_c5c7_3fb4);

/// A window / floater opening (`UISndWindowOpen`, and the incoming voice call,
/// which defaults to it).
pub const UI_SOUND_WINDOW_OPEN: Uuid = Uuid::from_u128(0xc802_60ba_41fd_8a46_768a_6bf2_3636_0e3a);

/// A window / floater closing (`UISndWindowClose`).
pub const UI_SOUND_WINDOW_CLOSE: Uuid = Uuid::from_u128(0x2c34_6eda_b60c_ab33_1119_b894_1916_a499);

/// A new IM session, an inventory offer, a teleport offer or a friendship
/// offer — the reference points `UISndNewIncomingIMSession`,
/// `UISndInventoryOffer`, `UISndTeleportOffer` and `UISndFriendshipOffer` at
/// one asset.
pub const UI_SOUND_IM_OR_OFFER: Uuid = Uuid::from_u128(0x67cc_2844_00f3_2b3c_b991_6418_d01e_1bb7);

/// A nearby-chat message arriving (`UISndNearbyChat`, and the radar alerts,
/// whose seven `UISndRadar*` settings all default to it).
pub const UI_SOUND_NEARBY_CHAT: Uuid = Uuid::from_u128(0xa3f4_8b85_c29f_1f97_ebb6_644b_7c05_3512);

/// Every built-in UI sound in one list, so a grid fixture can answer the whole
/// set and a test can prove none was forgotten — the sound counterpart of
/// [`BUILTIN_ENVIRONMENT_TEXTURES`](crate::BUILTIN_ENVIRONMENT_TEXTURES).
///
/// A viewer asks for the ones it is configured to play as soon as it has an
/// asset capability (the reference preloads its own set at login, so the fetch
/// is not paid at the moment the sound is wanted), and for the rest the first
/// time the event they belong to happens. Which of the two a given id falls
/// under is a viewer's own business; a grid has to be able to answer all of
/// them.
pub const BUILTIN_UI_SOUNDS: [Uuid; 12] = [
    UI_SOUND_CLICK,
    UI_SOUND_TYPING,
    UI_SOUND_ALERT,
    UI_SOUND_INVALID_OP,
    UI_SOUND_MONEY_UP,
    UI_SOUND_MONEY_DOWN,
    UI_SOUND_TELEPORT_OUT,
    UI_SOUND_SNAPSHOT,
    UI_SOUND_WINDOW_OPEN,
    UI_SOUND_WINDOW_CLOSE,
    UI_SOUND_IM_OR_OFFER,
    UI_SOUND_NEARBY_CHAT,
];

#[cfg(test)]
mod tests {
    use super::BUILTIN_UI_SOUNDS;
    use pretty_assertions::assert_eq;

    /// The list holds twelve *distinct* ids. A duplicate would mean a grid
    /// fixture answering one id twice and another not at all, which is exactly
    /// the failure the list exists to prevent.
    #[test]
    fn every_built_in_ui_sound_id_is_distinct() {
        let mut ids = BUILTIN_UI_SOUNDS.to_vec();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two built-in UI sounds share an id");
    }

    /// None of them is the nil id: a nil asset id is how the reference viewer
    /// spells "this sound is disabled", and a disabled sound in a list of
    /// assets to serve would have a grid registering bytes under a key nothing
    /// can ever ask for.
    #[test]
    fn no_built_in_ui_sound_is_nil() {
        for id in BUILTIN_UI_SOUNDS {
            assert!(!id.is_nil(), "a built-in UI sound is the nil id");
        }
    }
}
