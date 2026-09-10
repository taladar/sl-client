//! The viewer's live **RLV / RLVa** state, and the settings that steer it.
//!
//! [`sl_rlv`] is a pure crate: it decodes the `@`-command language, holds what
//! the held commands mean, and answers what a restriction allows — but it never
//! reads a viewer and never obeys anything. This module is where the viewer
//! keeps one of those state machines, so every surface consults the *same* one.
//!
//! It lives in the world-API tier for the reason the tier exists: the RLVa
//! floaters draw this state, but the chat bar, the session command path and the
//! wear paths all have to *ask* it, and none of those may depend on the UI crate
//! that draws it. The state is here; the systems that fill it and the windows
//! that show it stay with their own features.
//!
//! # The three things kept here
//!
//! - [`RlvSession`] — the [`RlvState`] machine itself, a revision counter that
//!   ticks whenever a command changes it (so a floater rebuilds only when there
//!   is something new to draw), the **console transcript** the RLVa console
//!   shows, and the **reply queue**: the lines the engine owes the grid, which
//!   three unrelated producers fill and one system sends.
//! - [`RLV_STRINGS`] — the canned texts RLVa emits (`rlva_strings.xml` in the
//!   reference). The eight the reference marks *customizable* are registered as
//!   settings, so the Strings floater edits them through the ordinary settings
//!   store rather than a file of its own.
//! - The **setting roster** — the whole `RlvSettingNames` list
//!   (`rlvdefines.h`), because the toggles the RLVa menu draws and the flags the
//!   enforcement layers read have to be the same names.
//!
//! # A deliberate divergence
//!
//! The reference writes customised strings to `rlv_strings.xml` and tells the
//! user they need a relog; here they are ordinary settings and take effect at
//! once. Nothing in the string table is read at startup only, so the relog was
//! an artefact of where the reference stored them, not a rule worth copying.

#![expect(
    clippy::module_name_repetitions,
    reason = "the module is named for the one protocol it owns, and RLV is what \
              its items are called everywhere else — in the reference, in \
              `sl-rlv`, and at every call site, which reads them as \
              `rlv::RlvSession` only in the import"
)]

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use sl_client_bevy::{
    ChatSource, ChatType, CloudPosDensity, Color, ColorAlpha, Glow, Key, ObjectKey, SettingsKind,
    SkySettings, TextureKey, Uuid, azimuth_altitude_to_rotation,
};
use sl_rlv::{
    RlvAttachmentPoint, RlvDebugSetting, RlvDebugValue, RlvEnvRequest, RlvEnvSource, RlvExtSource,
    RlvObjectAttachment, RlvReply, RlvSkyBody, RlvSkyField, RlvSkyValue, RlvState, is_rlv_line,
};
use sl_settings::SettingValue;
use sl_viewer_kit::coords::sky_body_direction;
use sl_viewer_settings::ViewerSettings;

use crate::{MAX_PARENT_WALK, ObjectState};

/// The whole-environment change an RLV `@setenv_*` command asks for, re-exported
/// so the scene that carries one out needs this seam and not the language crate
/// behind it.
pub use sl_rlv::RlvEnvRequest as RlvEnvironmentRequest;

/// Whether the user may still change their own environment, or an object holds
/// `@setenv` and has taken the menu away. Re-exported for the same reason: the
/// menu bar reads this seam, not the language crate.
pub use sl_rlv::can_change_environment;

/// The `[rlv]` section every RLV setting is grouped under in the settings file.
pub const RLV_SECTION: &[&str] = &["rlv"];

/// The `[rlv.strings]` section the customisable response strings live under.
pub const RLV_STRINGS_SECTION: &[&str] = &["rlv", "strings"];

// --- The setting roster (`RlvSettingNames`, rlvdefines.h) ------------------

/// `RestrainedLove` — the master switch. RLV is off until the user turns it on,
/// as in the reference, because an object that can restrain the viewer should
/// never be able to do so by surprise.
pub const SETTING_MAIN: &str = "RestrainedLove";

/// `RestrainedLoveDebug` — echo every processed command into the RLVa console.
/// Read by the command-intake path once an object can speak `@` to this viewer;
/// today only the console itself processes commands, and it echoes regardless.
pub const SETTING_DEBUG: &str = "RestrainedLoveDebug";

/// The menu condition key that holds while [`SETTING_DEBUG`] is on.
pub const COND_DEBUG: &str = "rlv-debug-on";

/// `RestrainedLoveCanOOC` — let `((out of character))` chat through `@sendchat`.
pub const SETTING_CAN_OOC: &str = "RestrainedLoveCanOOC";

/// The menu condition key that holds while [`SETTING_CAN_OOC`] is on.
pub const COND_CAN_OOC: &str = "rlv-can-ooc-on";

/// `RestrainedLoveForbidGiveToRLV` — refuse `llGiveInventory` folders offered
/// into the shared `#RLV` folder.
pub const SETTING_FORBID_GIVE_TO_RLV: &str = "RestrainedLoveForbidGiveToRLV";

/// The menu condition key that holds while [`SETTING_FORBID_GIVE_TO_RLV`] is on.
pub const COND_FORBID_GIVE_TO_RLV: &str = "rlv-forbid-give-to-rlv-on";

/// `RestrainedLoveNoSetEnv` — opt out of script control of the environment
/// entirely (the `@setenv_*` family).
pub const SETTING_NO_SET_ENV: &str = "RestrainedLoveNoSetEnv";

/// The menu condition key that holds while [`SETTING_NO_SET_ENV`] is on.
pub const COND_NO_SET_ENV: &str = "rlv-no-set-env-on";

/// `RestrainedLoveShowEllipsis` — say a blanked line as `"..."` rather than not
/// saying it at all.
pub const SETTING_SHOW_ELLIPSIS: &str = "RestrainedLoveShowEllipsis";

/// The menu condition key that holds while [`SETTING_SHOW_ELLIPSIS`] is on.
pub const COND_SHOW_ELLIPSIS: &str = "rlv-show-ellipsis-on";

/// `RestrainedLoveStackWhenFolderBeginsWith` — the shared-folder name prefix
/// that means "wear this **in addition to**" rather than "wear this instead of".
pub const SETTING_WEAR_ADD_PREFIX: &str = "RestrainedLoveStackWhenFolderBeginsWith";

/// `RestrainedLoveReplaceWhenFolderBeginsWith` — the shared-folder name prefix
/// that means "wear this **instead of**".
pub const SETTING_WEAR_REPLACE_PREFIX: &str = "RestrainedLoveReplaceWhenFolderBeginsWith";

/// `RLVaDebugHideUnsetDuplicate` — drop the console echo of a command that set
/// nothing new or lifted nothing that was held.
pub const SETTING_DEBUG_HIDE_UNSET_DUPLICATE: &str = "RLVaDebugHideUnsetDuplicate";

/// The menu condition key that holds while [`SETTING_DEBUG_HIDE_UNSET_DUPLICATE`] is on.
pub const COND_DEBUG_HIDE_UNSET_DUPLICATE: &str = "rlv-debug-hide-unset-duplicate-on";

/// `RLVaEnableIMQuery` — answer `@list` / `@except` asked over IM.
pub const SETTING_ENABLE_IM_QUERY: &str = "RLVaEnableIMQuery";

/// The menu condition key that holds while [`SETTING_ENABLE_IM_QUERY`] is on.
pub const COND_ENABLE_IM_QUERY: &str = "rlv-enable-im-query-on";

/// `RLVaEnableLegacyNaming` — use the legacy naming rules when anonymising a
/// resident's name.
pub const SETTING_ENABLE_LEGACY_NAMING: &str = "RLVaEnableLegacyNaming";

/// The menu condition key that holds while [`SETTING_ENABLE_LEGACY_NAMING`] is on.
pub const COND_ENABLE_LEGACY_NAMING: &str = "rlv-enable-legacy-naming-on";

/// `RLVaEnableSharedWear` — allow wearing from the shared `#RLV` folder by hand.
pub const SETTING_ENABLE_SHARED_WEAR: &str = "RLVaEnableSharedWear";

/// The menu condition key that holds while [`SETTING_ENABLE_SHARED_WEAR`] is on.
pub const COND_ENABLE_SHARED_WEAR: &str = "rlv-enable-shared-wear-on";

/// `RLVaEnableTemporaryAttachments` — let a script attach a temporary
/// attachment while RLV is on.
pub const SETTING_ENABLE_TEMP_ATTACH: &str = "RLVaEnableTemporaryAttachments";

/// The menu condition key that holds while [`SETTING_ENABLE_TEMP_ATTACH`] is on.
pub const COND_ENABLE_TEMP_ATTACH: &str = "rlv-enable-temporary-attachments-on";

/// `RLVaHideLockedLayers` — hide a locked clothing layer from the UI rather
/// than showing it greyed out.
pub const SETTING_HIDE_LOCKED_LAYERS: &str = "RLVaHideLockedLayers";

/// The menu condition key that holds while [`SETTING_HIDE_LOCKED_LAYERS`] is on.
pub const COND_HIDE_LOCKED_LAYERS: &str = "rlv-hide-locked-layers-on";

/// `RLVaHideLockedAttachments` — hide a locked attachment from the UI.
pub const SETTING_HIDE_LOCKED_ATTACHMENTS: &str = "RLVaHideLockedAttachments";

/// The menu condition key that holds while [`SETTING_HIDE_LOCKED_ATTACHMENTS`] is on.
pub const COND_HIDE_LOCKED_ATTACHMENTS: &str = "rlv-hide-locked-attachments-on";

/// `RLVaHideLockedInventory` — hide a locked item from the inventory listing.
pub const SETTING_HIDE_LOCKED_INVENTORY: &str = "RLVaHideLockedInventory";

/// The menu condition key that holds while [`SETTING_HIDE_LOCKED_INVENTORY`] is on.
pub const COND_HIDE_LOCKED_INVENTORY: &str = "rlv-hide-locked-inventory-on";

/// `RLVaLoginLastLocation` — allow logging in to the last location while RLV is
/// on (the reference makes this opt-in because a restraint may not want it).
pub const SETTING_LOGIN_LAST_LOCATION: &str = "RLVaLoginLastLocation";

/// The menu condition key that holds while [`SETTING_LOGIN_LAST_LOCATION`] is on.
pub const COND_LOGIN_LAST_LOCATION: &str = "rlv-login-last-location-on";

/// `RLVaSharedInvAutoRename` — rename an item dropped into `#RLV` so its name
/// carries the attachment point it was worn on.
pub const SETTING_SHARED_INV_AUTO_RENAME: &str = "RLVaSharedInvAutoRename";

/// The menu condition key that holds while [`SETTING_SHARED_INV_AUTO_RENAME`] is on.
pub const COND_SHARED_INV_AUTO_RENAME: &str = "rlv-shared-inv-auto-rename-on";

/// `RLVaShowAssertionFailures` — surface an internal RLVa assertion failure
/// instead of only logging it.
pub const SETTING_SHOW_ASSERTION_FAILURES: &str = "RLVaShowAssertionFailures";

/// The menu condition key that holds while [`SETTING_SHOW_ASSERTION_FAILURES`] is on.
pub const COND_SHOW_ASSERTION_FAILURES: &str = "rlv-show-assertion-failures-on";

/// `RLVaShowRedirectChatTyping` — keep sending the typing indicator while
/// `@redirchat` is in force. Off by default: a typing indicator with no chat
/// after it gives the redirect away.
pub const SETTING_SHOW_REDIRECT_CHAT_TYPING: &str = "RLVaShowRedirectChatTyping";

/// The menu condition key that holds while [`SETTING_SHOW_REDIRECT_CHAT_TYPING`] is on.
pub const COND_SHOW_REDIRECT_CHAT_TYPING: &str = "rlv-show-redirect-chat-typing-on";

/// `RLVaSplitRedirectChat` — split a redirected line too long for one chat
/// message across several rather than truncating it.
pub const SETTING_SPLIT_REDIRECT_CHAT: &str = "RLVaSplitRedirectChat";

/// The menu condition key that holds while [`SETTING_SPLIT_REDIRECT_CHAT`] is on.
pub const COND_SPLIT_REDIRECT_CHAT: &str = "rlv-split-redirect-chat-on";

/// `RLVaWearReplaceUnlocked` — wearing over a locked layer replaces the
/// unlocked items on it rather than refusing outright.
pub const SETTING_WEAR_REPLACE_UNLOCKED: &str = "RLVaWearReplaceUnlocked";

/// The menu condition key that holds while [`SETTING_WEAR_REPLACE_UNLOCKED`] is on.
pub const COND_WEAR_REPLACE_UNLOCKED: &str = "rlv-wear-replace-unlocked-on";

/// The menu condition key that holds while RLV itself is on — the gate every
/// other RLVa menu entry is enabled by, and the tick on the master toggle.
pub const RLV_ENABLED: &str = "rlv-enabled";

/// One boolean RLV setting: its name, its declared default, the one-line
/// description the debug-settings editor shows, and the **menu condition key**
/// its check mark is drawn from.
///
/// The condition key lives here rather than beside the menu so the toggle and
/// its tick cannot be spelled differently in two files. The reference's own
/// menu XML repeats the setting name in both the `on_check` and the `on_click`
/// for the same reason, and gets to skip the key only because its menu system
/// reads settings directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RlvFlagDef {
    /// The setting name (`RlvSettingNames`).
    pub name: &'static str,
    /// The reference's default.
    pub default: bool,
    /// What the setting does, one line.
    pub comment: &'static str,
    /// The menu condition key that holds while the flag is on.
    pub condition: &'static str,
}

/// The **boolean** settings, in the order the reference declares them.
///
/// A table rather than twenty-odd `register_in` calls because the RLVa menu has
/// to walk exactly this list to know which toggles exist, and two hand-written
/// copies of it would drift.
pub const RLV_BOOL_SETTINGS: &[RlvFlagDef] = &[
    RlvFlagDef {
        name: SETTING_MAIN,
        default: false,
        comment: "Obey RLV / RLVa commands from worn objects",
        condition: RLV_ENABLED,
    },
    RlvFlagDef {
        name: SETTING_DEBUG,
        default: false,
        comment: "Echo every processed RLV command into the RLVa console",
        condition: COND_DEBUG,
    },
    RlvFlagDef {
        name: SETTING_CAN_OOC,
        default: true,
        comment: "Let ((out of character)) chat through a chat restriction",
        condition: COND_CAN_OOC,
    },
    RlvFlagDef {
        name: SETTING_FORBID_GIVE_TO_RLV,
        default: false,
        comment: "Refuse inventory offers into the shared #RLV folder",
        condition: COND_FORBID_GIVE_TO_RLV,
    },
    RlvFlagDef {
        name: SETTING_NO_SET_ENV,
        default: false,
        comment: "Refuse script control of the environment (@setenv)",
        condition: COND_NO_SET_ENV,
    },
    RlvFlagDef {
        name: SETTING_SHOW_ELLIPSIS,
        default: true,
        comment: "Say a blanked line as \"...\" rather than saying nothing",
        condition: COND_SHOW_ELLIPSIS,
    },
    RlvFlagDef {
        name: SETTING_DEBUG_HIDE_UNSET_DUPLICATE,
        default: true,
        comment: "Hide the console echo of a command that changed nothing",
        condition: COND_DEBUG_HIDE_UNSET_DUPLICATE,
    },
    RlvFlagDef {
        name: SETTING_ENABLE_IM_QUERY,
        default: false,
        comment: "Answer @list and @except asked over instant message",
        condition: COND_ENABLE_IM_QUERY,
    },
    RlvFlagDef {
        name: SETTING_ENABLE_LEGACY_NAMING,
        default: false,
        comment: "Use the legacy naming rules when anonymising a resident",
        condition: COND_ENABLE_LEGACY_NAMING,
    },
    RlvFlagDef {
        name: SETTING_ENABLE_SHARED_WEAR,
        default: false,
        comment: "Allow wearing items from the shared #RLV folder by hand",
        condition: COND_ENABLE_SHARED_WEAR,
    },
    RlvFlagDef {
        name: SETTING_ENABLE_TEMP_ATTACH,
        default: true,
        comment: "Allow scripts to attach temporary attachments",
        condition: COND_ENABLE_TEMP_ATTACH,
    },
    RlvFlagDef {
        name: SETTING_HIDE_LOCKED_LAYERS,
        default: false,
        comment: "Hide a locked clothing layer instead of greying it out",
        condition: COND_HIDE_LOCKED_LAYERS,
    },
    RlvFlagDef {
        name: SETTING_HIDE_LOCKED_ATTACHMENTS,
        default: false,
        comment: "Hide a locked attachment instead of greying it out",
        condition: COND_HIDE_LOCKED_ATTACHMENTS,
    },
    RlvFlagDef {
        name: SETTING_HIDE_LOCKED_INVENTORY,
        default: false,
        comment: "Hide a locked item from the inventory listing",
        condition: COND_HIDE_LOCKED_INVENTORY,
    },
    RlvFlagDef {
        name: SETTING_LOGIN_LAST_LOCATION,
        default: true,
        comment: "Allow logging in to the last location while RLV is on",
        condition: COND_LOGIN_LAST_LOCATION,
    },
    RlvFlagDef {
        name: SETTING_SHARED_INV_AUTO_RENAME,
        default: true,
        comment: "Rename an item dropped into #RLV to carry its attachment point",
        condition: COND_SHARED_INV_AUTO_RENAME,
    },
    RlvFlagDef {
        name: SETTING_SHOW_ASSERTION_FAILURES,
        default: false,
        comment: "Surface an internal RLVa assertion failure, not only log it",
        condition: COND_SHOW_ASSERTION_FAILURES,
    },
    RlvFlagDef {
        name: SETTING_SHOW_REDIRECT_CHAT_TYPING,
        default: false,
        comment: "Keep sending the typing indicator while chat is redirected",
        condition: COND_SHOW_REDIRECT_CHAT_TYPING,
    },
    RlvFlagDef {
        name: SETTING_SPLIT_REDIRECT_CHAT,
        default: false,
        comment: "Split a long redirected line instead of truncating it",
        condition: COND_SPLIT_REDIRECT_CHAT,
    },
    RlvFlagDef {
        name: SETTING_WEAR_REPLACE_UNLOCKED,
        default: true,
        comment: "Wearing over a locked layer replaces the unlocked items on it",
        condition: COND_WEAR_REPLACE_UNLOCKED,
    },
];

/// The **string** settings that are not response strings: the two shared-folder
/// name prefixes, with the reference's defaults.
pub const RLV_PREFIX_SETTINGS: &[(&str, &str, &str)] = &[
    (
        SETTING_WEAR_ADD_PREFIX,
        "+",
        "Shared-folder name prefix meaning \"wear in addition to\"",
    ),
    (
        SETTING_WEAR_REPLACE_PREFIX,
        "-",
        "Shared-folder name prefix meaning \"wear instead of\"",
    ),
];

// --- The response strings (`rlva_strings.xml`) -----------------------------

/// One customisable RLVa response string: the key `sl-rlv` and the enforcement
/// layers name it by, the reference's default text, the label the Strings
/// floater lists it under, and the sentence describing when it is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RlvStringDef {
    /// The key — the setting name, and the name the code asks for.
    pub key: &'static str,
    /// The reference's text, and this setting's declared default.
    pub default: &'static str,
    /// The label the Strings floater lists this entry under.
    pub label: &'static str,
    /// When this string is emitted, shown under the label.
    pub description: &'static str,
}

/// The eight strings the reference marks `customizable` — the ones the Strings
/// floater lets the user rewrite, in **key** order.
///
/// Key order rather than label order because the key is what code names a
/// string by, so a sorted-by-key table is the one a reader can scan for the
/// name they have in hand; it also groups the family prefixes (`blocked_`,
/// `imquery_`, `stopim_`) together, which label order would scatter.
///
/// The rest of `rlva_strings.xml` (the `hidden_*` placeholders and the
/// `blocked_*` refusal notices) is not customisable there and is not here
/// either; those texts belong to the viewer's translated notification
/// catalogue rather than to a per-user override.
pub const RLV_STRINGS: &[RlvStringDef] = &[
    RlvStringDef {
        key: "blocked_recvim",
        default: "*** IM blocked by your viewer",
        label: "Blocked incoming IM message (local)",
        description: "Shown in place of the original message when an incoming IM is blocked",
    },
    RlvStringDef {
        key: "blocked_recvim_remote",
        default: "The Resident you messaged is currently prevented from reading your instant \
                  messages at the moment, please try again later.",
        label: "Blocked incoming IM message (remote)",
        description: "Sent to the remote party when their IM was blocked",
    },
    RlvStringDef {
        key: "blocked_sendim",
        default: "*** IM blocked by sender's viewer",
        label: "Blocked outgoing IM message (local + remote)",
        description: "Shown (and sent to the remote party) when an outgoing IM is blocked",
    },
    RlvStringDef {
        key: "blocked_tplurerequest_remote",
        default: "The Resident is currently prevented from accepting. Please try again later.",
        label: "Blocked teleport offer/request (remote)",
        description: "Sent to the remote party when their teleport offer or request was blocked",
    },
    RlvStringDef {
        key: "imquery_list_deny",
        default: "*** The other party respectfully requests you mind your own business (bunnies \
                  made me do it!)",
        label: "@list and @except command (remote)",
        description: "Sent to the remote party when you deny their request to list your active \
                      RLV restrictions",
    },
    RlvStringDef {
        key: "imquery_list_suffix",
        default: "(Use @except to see the list of active exceptions)",
        label: "@list command suffix (remote)",
        description: "Sent to the remote party as a suffix to @list to inform them how to request \
                      your exceptions",
    },
    RlvStringDef {
        key: "stopim_endsession_remote",
        default: "*** Session has been ended for the other party",
        label: "@stopim command with an active session (remote)",
        description: "Sent to the remote party when they attempt to forcefully close the IM \
                      conversation (and it exists)",
    },
    RlvStringDef {
        key: "stopim_nosession",
        default: "*** The other party is not under a @startim restriction",
        label: "@stopim command with no session (remote)",
        description: "Sent to the remote party when they attempt to forcefully close your IM \
                      conversation with them (and no such session exists)",
    },
];

/// Look up a response string's definition by key.
#[must_use]
pub fn rlv_string_def(key: &str) -> Option<&'static RlvStringDef> {
    RLV_STRINGS.iter().find(|entry| entry.key == key)
}

/// The live text of the response string `key` — the user's override where there
/// is one, the reference's default otherwise, and the key itself for a name no
/// table knows (which can only be a typo at a call site, and reads as one).
#[must_use]
pub fn rlv_string(settings: Option<&ViewerSettings>, key: &str) -> String {
    if let Some(settings) = settings
        && let Ok(text) = settings.store().get_str(key)
    {
        return text.to_owned();
    }
    rlv_string_def(key).map_or_else(|| key.to_owned(), |entry| entry.default.to_owned())
}

// --- The live state --------------------------------------------------------

/// Which stream a console line came out of, which is the whole of how it is
/// coloured and prefixed.
///
/// The reference has a third command stream, `RET:` for a command held back
/// until the viewer knows more. Our state machine has no such outcome — a
/// command is applied or refused there and then — so there is no stream that
/// could ever carry a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlvConsoleKind {
    /// A line the user typed, echoed back after the prompt.
    Input,
    /// A command that was accepted (`INFO:` in the reference).
    Info,
    /// A command that was refused (`ERR:`).
    Error,
    /// The answer to a `@get*` query, as the asking script would have heard it.
    Reply,
}

impl RlvConsoleKind {
    /// The prefix the reference's console writes ahead of this kind of line.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Input => "> ",
            Self::Info => "INFO: ",
            Self::Error => "ERR: ",
            Self::Reply => "",
        }
    }
}

/// One line of the RLVa console transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlvConsoleLine {
    /// Which stream it came from.
    pub kind: RlvConsoleKind,
    /// The text, without the [`RlvConsoleKind::prefix`].
    pub text: String,
}

/// How many console lines are kept. Old lines fall off the front, so a chatty
/// `RestrainedLoveDebug` session cannot grow without bound.
pub const RLV_CONSOLE_CAPACITY: usize = 512;

/// How many unsent replies are kept. A burst of `@notify` traffic with nothing
/// draining it (no session, a headless test) drops its oldest lines rather than
/// growing forever.
pub const RLV_REPLY_CAPACITY: usize = 256;

/// The viewer's one RLV state machine, its revision, the console transcript,
/// and the lines it owes the grid.
///
/// The revision is what a floater watches: `RlvState` is a plain value with no
/// change detection of its own inside the resource, and Bevy's `Res` change
/// detection fires on any mutable borrow — including the ones that only append
/// a console line. A counter bumped exactly where a command changed the held
/// set lets the Restrictions and Locks floaters rebuild only when there is
/// something new to show.
#[derive(Resource, Debug, Default)]
pub struct RlvSession {
    /// The state machine every surface consults.
    state: RlvState,
    /// Ticks whenever an applied command changed the held set.
    revision: u64,
    /// The console transcript, oldest first, capped at
    /// [`RLV_CONSOLE_CAPACITY`].
    console: VecDeque<RlvConsoleLine>,
    /// Ticks whenever a console line is appended or the transcript is cleared.
    console_revision: u64,
    /// The lines waiting to be shouted back at the objects that asked for them,
    /// oldest first. See [`push_reply`](Self::push_reply).
    replies: VecDeque<RlvReply>,
}

impl RlvSession {
    /// The state machine, for the surfaces that only read it.
    #[must_use]
    pub const fn state(&self) -> &RlvState {
        &self.state
    }

    /// The state machine, for the one system that drives it. Every caller that
    /// changes the held set must follow with [`bump`](Self::bump).
    pub const fn state_mut(&mut self) -> &mut RlvState {
        &mut self.state
    }

    /// The restriction revision — see the type documentation.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Record that the held set changed.
    pub const fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The console transcript, oldest first.
    #[must_use]
    pub const fn console(&self) -> &VecDeque<RlvConsoleLine> {
        &self.console
    }

    /// The console revision — ticks on every append and on a clear.
    #[must_use]
    pub const fn console_revision(&self) -> u64 {
        self.console_revision
    }

    /// Append one console line, dropping the oldest when the cap is reached.
    pub fn log(&mut self, kind: RlvConsoleKind, text: impl Into<String>) {
        if self.console.len() >= RLV_CONSOLE_CAPACITY {
            self.console.pop_front();
        }
        self.console.push_back(RlvConsoleLine {
            kind,
            text: text.into(),
        });
        self.console_revision = self.console_revision.wrapping_add(1);
    }

    /// Queue one line to be chatted back to the grid.
    ///
    /// Three unrelated parts of the engine build a line and cannot send it —
    /// the `@get*` answer ([`RlvState::answer`]), the `@notify` subscribers'
    /// reports ([`RlvState::take_notifications`]) and the `@getdebug_*` answer
    /// ([`RlvState::run_extension`]). They all hand back an
    /// [`RlvReply`] that is already truncated and channel-checked, so what they
    /// need is not three sends but **one**: they queue here, and the single
    /// system that owns the session's chat send drains it. A fourth producer
    /// added later inherits the send for free.
    ///
    /// Bounded like the console transcript, and for the same reason: a session
    /// with no chat send running (a headless test, a viewer between circuits)
    /// must not grow this without limit. The oldest line falls off, because a
    /// reply whose script has long since stopped waiting is the one worth
    /// losing.
    pub fn push_reply(&mut self, reply: RlvReply) {
        if self.replies.len() >= RLV_REPLY_CAPACITY {
            self.replies.pop_front();
        }
        self.replies.push_back(reply);
    }

    /// Whether anything is waiting in [`take_replies`](Self::take_replies) —
    /// the read the drain system does before taking the mutable borrow.
    #[must_use]
    pub fn has_replies(&self) -> bool {
        !self.replies.is_empty()
    }

    /// Take every queued reply, oldest first.
    #[must_use]
    pub fn take_replies(&mut self) -> Vec<RlvReply> {
        self.replies.drain(..).collect()
    }

    /// Release **everything** the RLV engine is holding: every object's
    /// restrictions, exceptions, locks, modifier slots and `@notify`
    /// subscriptions, and any reply not yet sent.
    ///
    /// This is what turning the `RestrainedLove` master switch off does. The
    /// reference has no equivalent because it does not need one: its switch
    /// takes effect on the next **restart**, and a restart is a fresh process
    /// with an empty state machine. Applying the switch at once means reaching
    /// that same state at once — otherwise "off" would leave a viewer still
    /// restrained by a collar, with the RLVa windows that could show it greyed
    /// out because RLV is off.
    ///
    /// The queued replies go with it. A `@notify` subscriber being told its
    /// restriction was lifted, by a viewer that has just stopped speaking RLV
    /// at all, is a message about a conversation that has ended; the
    /// reference's restart likewise tells nobody.
    ///
    /// The **console transcript is kept**: it is the log of what happened, and
    /// what just happened is part of it.
    ///
    /// So are the **blocked keywords**
    /// ([`RlvState::set_behaviour_blocked`]). Those are the user's own
    /// settings rather than anything an object held, and dropping them here
    /// would quietly un-block `@setenv` for the next device the moment RLV was
    /// switched off and on again.
    pub fn release_all(&mut self) {
        let held = self.state.restricting_objects().next().is_some();
        let blocked: Vec<_> = self.state.blocked_behaviours().collect();
        self.state = RlvState::new();
        for (keyword, kind) in blocked {
            let _known = self.state.set_behaviour_blocked(keyword, kind, true);
        }
        self.replies.clear();
        // Only a real release is a change the floaters need to redraw for.
        if held {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Empty the transcript (the console's Clear button).
    pub fn clear_console(&mut self) {
        if self.console.is_empty() {
            return;
        }
        self.console.clear();
        self.console_revision = self.console_revision.wrapping_add(1);
    }
}

// --- The `@getdebug_*` / `@setdebug_*` allowlist ----------------------------

/// The two facts behind the **pseudo** debug settings — the ones with no
/// stored value for [`ViewerRlvExt`] to read.
///
/// Each is published by the feature that owns it (the camera publishes the
/// view's shape, the avatar layer publishes the shape of the avatar), because
/// neither belongs to the RLV surface that answers with them. `None` means
/// not known yet, which a script hears as an empty answer rather than as a
/// guess.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct RlvExtFacts {
    /// The 3D view's width divided by its height (`AspectRatio`).
    pub aspect_ratio: Option<f32>,
    /// Whether the worn Shape's `male` param says male (`AvatarSex`).
    pub avatar_is_male: Option<bool>,
}

/// The viewer behind the debug-setting allowlist: the settings store for the
/// rows that are real settings, [`RlvExtFacts`] for the two that are not.
///
/// Borrowed for the length of one command rather than kept, because both halves
/// are Bevy resources. The store is borrowed *immutably* because nothing here
/// is writable — see [`set_debug_value`](RlvExtSource::set_debug_value).
#[derive(Debug)]
pub struct ViewerRlvExt<'settings> {
    /// The settings store the readable RLV rows live in, where there is one.
    pub settings: Option<&'settings ViewerSettings>,
    /// The two computed facts.
    pub facts: RlvExtFacts,
}

impl RlvExtSource for ViewerRlvExt<'_> {
    /// What each allowlisted row reads as here.
    ///
    /// Two of the six answer nothing, and say why:
    ///
    /// - **`RenderResolutionDivisor`** — this viewer does not render at a
    ///   reduced resolution, so there is no setting to read and none to write.
    ///   It is the cheap vision impairment a collar reaches for, and it belongs
    ///   with the rest of the vision-restriction rendering rather than being
    ///   faked with a setting nothing looks at;
    /// - **`AvatarSex`** and **`AspectRatio`** answer nothing until the avatar
    ///   layer and the camera have published them.
    ///
    /// `WindLightUseAtmosShaders` is not a setting here either, but its answer
    /// is not in doubt: this viewer always renders the atmospheric sky, which
    /// is exactly what a script asking the question wants to know.
    fn debug_value(&self, setting: RlvDebugSetting) -> Option<RlvDebugValue> {
        match setting {
            RlvDebugSetting::AvatarSex => self.facts.avatar_is_male.map(RlvDebugValue::Bool),
            RlvDebugSetting::AspectRatio => self.facts.aspect_ratio.map(RlvDebugValue::Float),
            RlvDebugSetting::RenderResolutionDivisor => None,
            RlvDebugSetting::ForbidGiveToRlv => Some(RlvDebugValue::Bool(rlv_flag(
                self.settings,
                SETTING_FORBID_GIVE_TO_RLV,
            ))),
            RlvDebugSetting::NoSetEnv => Some(RlvDebugValue::Bool(rlv_flag(
                self.settings,
                SETTING_NO_SET_ENV,
            ))),
            RlvDebugSetting::WindLightUseAtmosShaders => Some(RlvDebugValue::Bool(true)),
        }
    }

    /// Nothing here is writable: of the two rows the allowlist lets a script
    /// write, `AvatarSex` is a pseudo setting the state machine keeps and
    /// `RenderResolutionDivisor` is the one this viewer does not have.
    fn set_debug_value(&mut self, _setting: RlvDebugSetting, _value: RlvDebugValue) -> bool {
        false
    }
}

// --- The `@getenv_*` / `@setenv_*` sky -------------------------------------

/// The seam between the RLV engine and the scene's environment layer.
///
/// [`sl_rlv`] owns the language — which subkeys exist, what each is scaled by,
/// how a colour is spelled — and asks an [`RlvEnvSource`] for the sky behind it.
/// The sky itself belongs to the scene, three crates away and unreachable from
/// the RLV surface, so this resource is the meeting point: the scene publishes
/// what it renders into [`rendered`](Self::rendered), the RLV engine edits
/// [`edited`](Self::edited) and queues whole-environment changes in
/// [`request`](Self::request), and the scene picks both up on its next frame.
///
/// It is a **holding cell, not a second copy of the truth**: nothing renders
/// from it, and the scene overwrites `rendered` from whatever it is actually
/// drawing. `edited` is deliberately sticky — it is the reference's `ENV_LOCAL`
/// layer, which outlives a region change and is dropped only by
/// `@setenv_daytime:-1` or the user going back to the shared environment.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct RlvEnvironmentSlot {
    /// The sky the scene is rendering, republished whenever it changes. `None`
    /// before the first environment is ingested, which is the one state a read
    /// cannot be answered from.
    pub rendered: Option<SkySettings>,
    /// The sky a script has edited, waiting for the scene to install it as the
    /// local environment layer. Cloned from [`rendered`](Self::rendered) on the
    /// first write, so a script never edits the region's own settings.
    pub edited: Option<SkySettings>,
    /// A whole-environment change (`@setenv_asset`, `@setenv_preset`,
    /// `@setenv_daycycle`, `@setenv_daytime`) for the scene to carry out and
    /// take.
    pub request: Option<RlvEnvRequest>,
    /// Whether the scene has a fixed sky pinned rather than a day cycle
    /// running — the one fact `@getenv_daytime` reports.
    pub fixed_sky: bool,
    /// The library `Environments` folder's settings assets by name — what
    /// `@setenv_preset:<name>` and `@setenv_daycycle:<name>` resolve against.
    /// Published by the inventory crate's settings index; empty until the
    /// library folder has been fetched, which is the same state the reference
    /// is in on a grid whose library has no such folder.
    pub library_environments: RlvLibraryEnvironments,
}

/// The library `Environments` folder, indexed the way `@setenv_preset` and
/// `@setenv_daycycle` search it: by settings kind and **case-insensitively** by
/// name (`RlvIsOfSettingsType`'s `boost::iequals`), first match winning.
///
/// It is a projection of the inventory mirror, not a second copy of it: only the
/// three fields a name lookup needs, published by
/// `sl_viewer_inventory::settings_index` whenever the mirror changes. It lives
/// on [`RlvEnvironmentSlot`] because [`RlvEnvSource::apply_environment`] is
/// called synchronously from inside the command parser, which holds no world.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RlvLibraryEnvironments {
    /// Keyed by kind and lower-cased name; the value is the settings **asset**
    /// id, which is what `@setenv_asset` would have been given directly.
    by_kind_and_name: HashMap<(SettingsKind, String), Uuid>,
}

impl RlvLibraryEnvironments {
    /// Record one library settings item, **keeping the first** of a repeated
    /// name: the reference takes `items.front()` of its collector's results, so
    /// a later duplicate never displaces an earlier one.
    pub fn insert(&mut self, kind: SettingsKind, name: &str, asset: Uuid) {
        self.by_kind_and_name
            .entry((kind, name.to_lowercase()))
            .or_insert(asset);
    }

    /// The asset a name of that kind resolves to, or `None` for a name the
    /// library does not carry — which is the reference's `RLV_RET_FAILED_OPTION`.
    #[must_use]
    pub fn resolve(&self, kind: SettingsKind, name: &str) -> Option<Uuid> {
        self.by_kind_and_name
            .get(&(kind, name.to_lowercase()))
            .copied()
    }

    /// How many named assets are indexed — the `Environments` folder being
    /// absent, unfetched or empty are all zero here.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_kind_and_name.len()
    }

    /// Whether nothing is indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_kind_and_name.is_empty()
    }
}

impl RlvEnvironmentSlot {
    /// The sky a read answers from: the edited one while a script holds it,
    /// otherwise what the scene is rendering.
    #[must_use]
    fn readable(&self) -> Option<&SkySettings> {
        self.edited.as_ref().or(self.rendered.as_ref())
    }

    /// The sky a write goes into, cloning the rendered one into the local layer
    /// on the first write as `RlvEnvironment::getTargetSky(true)` does.
    /// `None` when there is nothing to clone.
    fn writable(&mut self) -> Option<&mut SkySettings> {
        if self.edited.is_none() {
            self.edited = self.rendered.clone();
        }
        self.edited.as_mut()
    }

    /// Resolve the text of a `@setenv_preset` / `@setenv_daycycle` to a settings
    /// asset the way `fnApplyLibraryPreset` does: as an id when it parses as a
    /// non-nil one, otherwise by name against the library `Environments` folder
    /// for that kind.
    ///
    /// The literal nil id is *searched for by name*, not applied: the reference
    /// builds an `LLUUID` from the text and cannot tell "did not parse" from
    /// "parsed as null", so both fall through to the name search — which no
    /// library item answers, leaving the refusal where it belongs.
    #[must_use]
    fn resolve_settings(&self, text: &str, kind: SettingsKind) -> Option<Uuid> {
        match Uuid::try_parse(text) {
            Ok(id) if !id.is_nil() => Some(id),
            _ => self.library_environments.resolve(kind, text),
        }
    }
}

/// A sky texture id as RLV reads it: the null id for a sky that names none,
/// which is what the reference's `getSunTextureId` answers there.
#[must_use]
fn texture_id(texture: Option<TextureKey>) -> RlvSkyValue {
    RlvSkyValue::Texture(texture.map_or_else(Uuid::nil, |key| key.0.0))
}

/// A texture id a script wrote, as the sky stores it: the null id means "the
/// viewer's own default", which is `None` here.
#[must_use]
fn texture_key(id: Uuid) -> Option<TextureKey> {
    (!id.is_nil()).then_some(TextureKey(Key(id)))
}

impl RlvEnvSource for RlvEnvironmentSlot {
    /// Read one field of the sky.
    ///
    /// Every arm answers in the sky's own units; the scaling a script sees is
    /// [`sl_rlv`]'s and is applied on the other side of this call.
    fn sky_value(&self, field: RlvSkyField) -> Option<RlvSkyValue> {
        let sky = self.readable()?;
        Some(match field {
            RlvSkyField::Ambient => color(sky.ambient),
            RlvSkyField::BlueDensity => color(sky.blue_density),
            RlvSkyField::BlueHorizon => color(sky.blue_horizon),
            RlvSkyField::CloudColor => color(sky.cloud_color),
            RlvSkyField::CloudPosDensity1 => cloud_pos_density(sky.cloud_pos_density1),
            RlvSkyField::CloudPosDensity2 => cloud_pos_density(sky.cloud_pos_density2),
            // The alpha is not a channel RLV knows about; it is kept on write.
            RlvSkyField::SunlightColor => RlvSkyValue::Color([
                sky.sunlight_color.red(),
                sky.sunlight_color.green(),
                sky.sunlight_color.blue(),
            ]),
            RlvSkyField::Glow => {
                RlvSkyValue::Color([sky.glow.size(), sky.glow.reserved(), sky.glow.focus()])
            }
            RlvSkyField::CloudScrollRate => RlvSkyValue::Vec2(sky.cloud_scroll_rate),
            RlvSkyField::DensityMultiplier => RlvSkyValue::Float(sky.density_multiplier),
            RlvSkyField::DistanceMultiplier => RlvSkyValue::Float(sky.distance_multiplier),
            RlvSkyField::DropletRadius => RlvSkyValue::Float(sky.droplet_radius),
            RlvSkyField::HazeDensity => RlvSkyValue::Float(sky.haze_density),
            RlvSkyField::HazeHorizon => RlvSkyValue::Float(sky.haze_horizon),
            RlvSkyField::IceLevel => RlvSkyValue::Float(sky.ice_level),
            RlvSkyField::MaxY => RlvSkyValue::Float(sky.max_y),
            RlvSkyField::MoistureLevel => RlvSkyValue::Float(sky.moisture_level),
            RlvSkyField::Gamma => RlvSkyValue::Float(sky.gamma),
            RlvSkyField::CloudShadow => RlvSkyValue::Float(sky.cloud_shadow),
            RlvSkyField::CloudScale => RlvSkyValue::Float(sky.cloud_scale),
            RlvSkyField::CloudVariance => RlvSkyValue::Float(sky.cloud_variance),
            RlvSkyField::MoonBrightness => RlvSkyValue::Float(sky.moon_brightness),
            RlvSkyField::MoonScale => RlvSkyValue::Float(sky.moon_scale),
            RlvSkyField::SunScale => RlvSkyValue::Float(sky.sun_scale),
            RlvSkyField::StarBrightness => RlvSkyValue::Float(sky.star_brightness),
            RlvSkyField::CloudTexture => texture_id(sky.cloud_texture),
            RlvSkyField::MoonTexture => texture_id(sky.moon_texture),
            RlvSkyField::SunTexture => texture_id(sky.sun_texture),
        })
    }

    /// Write one field of the local sky.
    ///
    /// A value of the wrong shape for its field is refused rather than coerced —
    /// [`sl_rlv`] never sends one, and a viewer that silently accepted the wrong
    /// arm would hide the day it did.
    fn set_sky_value(&mut self, field: RlvSkyField, value: RlvSkyValue) -> bool {
        let Some(sky) = self.writable() else {
            return false;
        };
        match (field, value) {
            (RlvSkyField::Ambient, RlvSkyValue::Color(rgb)) => sky.ambient = sl_color(rgb),
            (RlvSkyField::BlueDensity, RlvSkyValue::Color(rgb)) => sky.blue_density = sl_color(rgb),
            (RlvSkyField::BlueHorizon, RlvSkyValue::Color(rgb)) => sky.blue_horizon = sl_color(rgb),
            (RlvSkyField::CloudColor, RlvSkyValue::Color(rgb)) => sky.cloud_color = sl_color(rgb),
            (RlvSkyField::CloudPosDensity1, RlvSkyValue::Color([x, y, density])) => {
                sky.cloud_pos_density1 = CloudPosDensity::new(x, y, density);
            }
            (RlvSkyField::CloudPosDensity2, RlvSkyValue::Color([x, y, density])) => {
                sky.cloud_pos_density2 = CloudPosDensity::new(x, y, density);
            }
            (RlvSkyField::SunlightColor, RlvSkyValue::Color([red, green, blue])) => {
                sky.sunlight_color = ColorAlpha::new(red, green, blue, sky.sunlight_color.alpha());
            }
            (RlvSkyField::Glow, RlvSkyValue::Color([size, reserved, focus])) => {
                sky.glow = Glow::new(size, reserved, focus);
            }
            (RlvSkyField::CloudScrollRate, RlvSkyValue::Vec2(pair)) => {
                sky.cloud_scroll_rate = pair;
            }
            (RlvSkyField::DensityMultiplier, RlvSkyValue::Float(value)) => {
                sky.density_multiplier = value;
            }
            (RlvSkyField::DistanceMultiplier, RlvSkyValue::Float(value)) => {
                sky.distance_multiplier = value;
            }
            (RlvSkyField::DropletRadius, RlvSkyValue::Float(value)) => sky.droplet_radius = value,
            (RlvSkyField::HazeDensity, RlvSkyValue::Float(value)) => sky.haze_density = value,
            (RlvSkyField::HazeHorizon, RlvSkyValue::Float(value)) => sky.haze_horizon = value,
            (RlvSkyField::IceLevel, RlvSkyValue::Float(value)) => sky.ice_level = value,
            (RlvSkyField::MaxY, RlvSkyValue::Float(value)) => sky.max_y = value,
            (RlvSkyField::MoistureLevel, RlvSkyValue::Float(value)) => sky.moisture_level = value,
            (RlvSkyField::Gamma, RlvSkyValue::Float(value)) => sky.gamma = value,
            (RlvSkyField::CloudShadow, RlvSkyValue::Float(value)) => sky.cloud_shadow = value,
            (RlvSkyField::CloudScale, RlvSkyValue::Float(value)) => sky.cloud_scale = value,
            (RlvSkyField::CloudVariance, RlvSkyValue::Float(value)) => sky.cloud_variance = value,
            (RlvSkyField::MoonBrightness, RlvSkyValue::Float(value)) => sky.moon_brightness = value,
            (RlvSkyField::MoonScale, RlvSkyValue::Float(value)) => sky.moon_scale = value,
            (RlvSkyField::SunScale, RlvSkyValue::Float(value)) => sky.sun_scale = value,
            (RlvSkyField::StarBrightness, RlvSkyValue::Float(value)) => sky.star_brightness = value,
            (RlvSkyField::CloudTexture, RlvSkyValue::Texture(id)) => {
                sky.cloud_texture = texture_key(id);
            }
            (RlvSkyField::MoonTexture, RlvSkyValue::Texture(id)) => {
                sky.moon_texture = texture_key(id);
            }
            (RlvSkyField::SunTexture, RlvSkyValue::Texture(id)) => {
                sky.sun_texture = texture_key(id);
            }
            _ => return false,
        }
        true
    }

    /// Where the sun or moon sits — its rotation applied to the Second Life `+X`
    /// axis, which is the form [`sl_rlv`] takes the spherical angles from.
    fn sky_direction(&self, body: RlvSkyBody) -> Option<[f32; 3]> {
        let sky = self.readable()?;
        Some(sky_body_direction(match body {
            RlvSkyBody::Sun => &sky.sun_rotation,
            RlvSkyBody::Moon => &sky.moon_rotation,
        }))
    }

    /// Point the sun or moon at the given spherical angles, through the
    /// reference's own `convert_azimuth_and_altitude_to_quat`.
    fn set_sky_angles(&mut self, body: RlvSkyBody, azimuth: f32, elevation: f32) -> bool {
        let rotation = azimuth_altitude_to_rotation(azimuth, elevation);
        let Some(sky) = self.writable() else {
            return false;
        };
        match body {
            RlvSkyBody::Sun => sky.sun_rotation = rotation,
            RlvSkyBody::Moon => sky.moon_rotation = rotation,
        }
        true
    }

    /// Queue a whole-environment change for the scene.
    ///
    /// `@setenv_preset` and `@setenv_daycycle` take **either** an asset id or a
    /// name, and the reference's `fnApplyLibraryPreset` tries them in that
    /// order: the text as an id first — applied exactly as `@setenv_asset`
    /// would be — and only a text that is no id searched by name against the
    /// inventory Library's `Environments` folder. The two commands search
    /// different kinds (`preset` is `ST_SKY`, `daycycle` is `ST_DAYCYCLE`), so
    /// they cannot share an arm; a name neither the library nor the parser
    /// claims is refused with the reference's `RLV_RET_FAILED_OPTION`.
    ///
    /// Any per-value edit still waiting is **dropped**: this replaces the whole
    /// local layer, so the sky that edit was building no longer has a layer to
    /// be the top of. That is the reference's own outcome for the two commands
    /// in that order — it installs the edited sky and then replaces it — reached
    /// without having to install the sky first.
    fn apply_environment(&mut self, request: &RlvEnvRequest) -> bool {
        let resolved = match *request {
            RlvEnvRequest::Asset(id) => RlvEnvRequest::Asset(id),
            RlvEnvRequest::Preset(ref text) => {
                let Some(id) = self.resolve_settings(text, SettingsKind::Sky) else {
                    return false;
                };
                RlvEnvRequest::Asset(id)
            }
            RlvEnvRequest::DayCycle(ref text) => {
                let Some(id) = self.resolve_settings(text, SettingsKind::DayCycle) else {
                    return false;
                };
                RlvEnvRequest::Asset(id)
            }
            RlvEnvRequest::DayTime(position) => RlvEnvRequest::DayTime(position),
            RlvEnvRequest::Clear => RlvEnvRequest::Clear,
        };
        self.edited = None;
        self.request = Some(resolved);
        true
    }

    /// Whether a fixed sky is pinned, as the scene last published.
    fn has_fixed_sky(&self) -> bool {
        self.fixed_sky
    }
}

/// A sky colour as the three channels [`sl_rlv`] works in.
#[must_use]
const fn color(value: Color) -> RlvSkyValue {
    RlvSkyValue::Color([value.red(), value.green(), value.blue()])
}

/// The inverse of [`color`].
#[must_use]
const fn sl_color([red, green, blue]: [f32; 3]) -> Color {
    Color::new(red, green, blue)
}

/// A cloud layer's position and density, which the reference stores and
/// addresses as a colour.
#[must_use]
const fn cloud_pos_density(value: CloudPosDensity) -> RlvSkyValue {
    RlvSkyValue::Color([value.position_x(), value.position_y(), value.density()])
}

// --- Settings --------------------------------------------------------------

/// Declare every RLV setting: the boolean roster, the two shared-folder name
/// prefixes, and the eight customisable response strings.
pub fn register_settings(settings: &mut ViewerSettings) {
    for flag in RLV_BOOL_SETTINGS {
        settings.register_in(
            RLV_SECTION,
            flag.name,
            SettingValue::Bool(flag.default),
            flag.comment,
        );
    }
    for (name, default, comment) in RLV_PREFIX_SETTINGS {
        settings.register_in(
            RLV_SECTION,
            name,
            SettingValue::String((*default).to_owned()),
            comment,
        );
    }
    for entry in RLV_STRINGS {
        settings.register_in(
            RLV_STRINGS_SECTION,
            entry.key,
            SettingValue::String(entry.default.to_owned()),
            entry.description,
        );
    }
}

/// Whether RLV is on — the `RestrainedLove` master switch. Everything else in
/// the family is gated on it, so an object that speaks `@` to a viewer with the
/// switch off is simply not heard.
#[must_use]
pub fn rlv_is_enabled(settings: Option<&ViewerSettings>) -> bool {
    settings.is_some_and(|settings| settings.store().get_bool(SETTING_MAIN).unwrap_or(false))
}

/// Read one RLV boolean setting, falling back to the roster's declared default
/// (and to `false` for a name the roster does not know, which can only be a
/// typo).
#[must_use]
pub fn rlv_flag(settings: Option<&ViewerSettings>, name: &str) -> bool {
    let default = RLV_BOOL_SETTINGS
        .iter()
        .find(|flag| flag.name == name)
        .is_some_and(|flag| flag.default);
    settings.map_or(default, |settings| {
        settings.store().get_bool(name).unwrap_or(default)
    })
}

/// A Bevy run condition for "RLV is enabled", for a system that must not run at
/// all while the master switch is off.
#[must_use]
pub fn rlv_enabled(settings: Option<Res<ViewerSettings>>) -> bool {
    rlv_is_enabled(settings.as_deref())
}

/// Whether `line` is an RLV command line the viewer should swallow rather than
/// show — re-exported from [`sl_rlv`] so a consumer of this module does not have
/// to name the pure crate for the one predicate it needs.
#[must_use]
pub fn is_rlv_command_line(line: &str) -> bool {
    is_rlv_line(line)
}

/// Whether an arriving chat line is an object speaking `@`-commands at this
/// viewer, and so is taken by the RLV engine instead of being shown.
///
/// This is the **whole** of the reference's admission test
/// (`llviewermessage.cpp:3142`), and it is deliberately loose:
///
/// - the chat is `CHAT_TYPE_OWNER`, which only `llOwnerSay` and
///   `llRegionSayTo` produce and which the **simulator** delivers only to the
///   object's owner. That is the security boundary, and it is drawn on the
///   server: somebody else's furniture cannot reach this agent's chat as
///   owner-say at all, which is why the reference does not re-check ownership
///   here and why this must not invent a check that only looks like safety;
/// - the line starts with `@`.
///
/// There is no general attachment test. A rezzed in-world prim the agent owns —
/// a bed, a cage, a cuff-post — commands the viewer directly and always has; a
/// club's poseball cannot, which is exactly why *relays* exist, and a relay is
/// worn, owned, and indistinguishable from any other worn object down here.
///
/// The **one** speaker the reference's or-chain excludes is a *temporary*
/// attachment (one a script attached, [`is_temp_attachment`]) while
/// `RLVaEnableTemporaryAttachments` is off. The chain's shape is what matters:
/// every other speaker is admitted whatever that flag says, and a viewer that
/// inverted it would refuse every ordinary collar. A speaker this viewer has
/// not streamed is admitted too, which is the reference's `(!chatter) || …`
/// first clause — an object whose update has not arrived is not evidence of
/// anything.
///
/// Every surface that displays or records nearby chat asks this, so the line
/// the engine takes cannot leak out of one of them — and the refusal is the
/// same test as the swallow, so a line the gate refuses is one they *show*.
#[must_use]
pub fn swallows_owner_say(
    settings: Option<&ViewerSettings>,
    objects: Option<&ObjectState>,
    source: ChatSource,
    chat_type: ChatType,
    message: &str,
) -> bool {
    rlv_is_enabled(settings)
        && chat_type == ChatType::Owner
        && is_rlv_line(message)
        && (rlv_flag(settings, SETTING_ENABLE_TEMP_ATTACH)
            || !source
                .object_key()
                .zip(objects)
                .is_some_and(|(key, objects)| is_temp_attachment(objects, key)))
}

/// Whether the object with grid-wide key `key` is a **temporary** attachment:
/// one a script attached (`llAttachToAvatarTemp`) rather than one the agent
/// wore from inventory.
///
/// The reference's test (`LLViewerObject::isTempAttachment`) is that the
/// object's own id and the `AttachItemID` the simulator sent for it are the
/// same — which is what a simulator sends when there is no inventory item to
/// name. `false` for an object this viewer has not streamed, and `false` for
/// an attachment that named no item at all, both matching the reference.
///
/// Unlike [`object_attachment`] this does **not** chase the linkset up to its
/// attachment root, because the reference does not either: it asks the
/// *speaking* object, and a child prim of an attachment carries neither an
/// attachment point nor an `AttachItemID` of its own, so a script talking from
/// one is admitted there as it is here.
#[must_use]
pub fn is_temp_attachment(objects: &ObjectState, key: ObjectKey) -> bool {
    objects.objects.values().any(|tracked| {
        tracked.full_key == key
            && tracked.attachment_point.is_some()
            && tracked.attachment_item == Some(key.uuid())
    })
}

/// Where the object with grid-wide key `key` sits on the avatar, or `None` when
/// it is not (part of) a worn attachment this viewer has streamed.
///
/// The prim that *speaks* is often a child of the attachment, and a bare
/// `@detach=n` locks the **attachment**, not the prim — so this chases the
/// linkset up to the first ancestor carrying an attachment point (the reference
/// takes `pObj->getRootEdit()->getID()` for the same reason) and reports that
/// root with its point. The walk is bounded exactly like
/// [`ObjectState::wearer_of`]'s, against a malformed parent cycle.
///
/// The intake caches the answer on the state machine
/// ([`RlvState::set_object_attachment`]) the first time it resolves, because
/// `@detach=y` may well arrive after the object is gone and there would be
/// nothing left to ask.
#[must_use]
pub fn object_attachment(objects: &ObjectState, key: ObjectKey) -> Option<RlvObjectAttachment> {
    let mut current = objects
        .objects
        .iter()
        .find(|(_scoped, tracked)| tracked.full_key == key)
        .map(|(scoped, _tracked)| *scoped)?;
    for _step in 0..MAX_PARENT_WALK {
        let tracked = objects.objects.get(&current)?;
        if let Some(point) = tracked.attachment_point {
            return RlvAttachmentPoint::from_index(point)
                .map(|point| RlvObjectAttachment::new(tracked.full_key.uuid(), point));
        }
        if tracked.is_root {
            return None;
        }
        current = tracked.parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        RLV_BOOL_SETTINGS, RLV_CONSOLE_CAPACITY, RLV_PREFIX_SETTINGS, RLV_REPLY_CAPACITY,
        RLV_STRINGS, RlvConsoleKind, RlvEnvironmentSlot, RlvLibraryEnvironments, RlvSession,
        rlv_flag, rlv_string, rlv_string_def, swallows_owner_say,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ChatSource, ChatType, ObjectKey, SettingsKind, SkySettings, Uuid};
    use sl_rlv::{
        RlvEnvRequest, RlvEnvSource as _, RlvNoFacts, RlvSkyBody, RlvSkyField, RlvSkyValue,
        parse_chat_line,
    };
    use std::collections::HashSet;

    /// Every setting name in the three rosters is unique — a duplicate would
    /// register twice and the second registration would be dropped with a
    /// warning nobody reads.
    #[test]
    fn every_setting_name_is_declared_once() {
        let mut names: Vec<&str> = RLV_BOOL_SETTINGS
            .iter()
            .map(|flag| flag.name)
            .chain(
                RLV_PREFIX_SETTINGS
                    .iter()
                    .map(|(name, _default, _comment)| *name),
            )
            .chain(RLV_STRINGS.iter().map(|entry| entry.key))
            .collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "an RLV setting name is declared twice");
    }

    /// The reference's own defaults, spot-checked where the default is the
    /// interesting half: RLV is off until asked for, and the two chat courtesies
    /// are on.
    #[test]
    fn the_master_switch_is_off_and_the_chat_courtesies_are_on() {
        assert!(!rlv_flag(None, super::SETTING_MAIN));
        assert!(rlv_flag(None, super::SETTING_CAN_OOC));
        assert!(rlv_flag(None, super::SETTING_SHOW_ELLIPSIS));
        assert!(!rlv_flag(None, super::SETTING_SPLIT_REDIRECT_CHAT));
        assert!(!rlv_flag(None, super::SETTING_SHOW_REDIRECT_CHAT_TYPING));
    }

    /// Every flag's menu condition key is its own, so no two toggles can tick
    /// each other.
    #[test]
    fn every_flag_has_its_own_menu_condition() {
        let keys: HashSet<&str> = RLV_BOOL_SETTINGS
            .iter()
            .map(|flag| flag.condition)
            .collect();
        assert_eq!(keys.len(), RLV_BOOL_SETTINGS.len());
        assert!(
            RLV_BOOL_SETTINGS
                .iter()
                .all(|flag| !flag.condition.is_empty())
        );
    }

    /// A name no roster knows reads as `false` rather than panicking — the
    /// shape a typo at a call site takes.
    #[test]
    fn an_unknown_flag_is_false() {
        assert!(!rlv_flag(None, "RLVaNoSuchSetting"));
    }

    /// With no settings store the response strings are the reference's texts,
    /// and an unknown key comes back as itself rather than empty, so a typo is
    /// visible in the surface that printed it.
    #[test]
    fn strings_fall_back_to_the_reference_text() {
        assert_eq!(
            rlv_string(None, "blocked_recvim"),
            "*** IM blocked by your viewer"
        );
        assert_eq!(rlv_string(None, "no_such_string"), "no_such_string");
        assert!(rlv_string_def("blocked_recvim").is_some());
        assert!(rlv_string_def("no_such_string").is_none());
    }

    /// The table is in key order, which is what the Strings floater lists in —
    /// so the picker's index is the table's index and neither has to sort.
    #[test]
    fn the_string_table_is_in_key_order() {
        let keys: Vec<&str> = RLV_STRINGS.iter().map(|entry| entry.key).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    /// The transcript keeps the newest lines and drops the oldest, and the
    /// revision ticks on every append.
    #[test]
    fn the_console_is_a_bounded_ring() {
        let mut session = RlvSession::default();
        for index in 0..RLV_CONSOLE_CAPACITY + 10 {
            session.log(RlvConsoleKind::Info, format!("line {index}"));
        }
        assert_eq!(session.console().len(), RLV_CONSOLE_CAPACITY);
        assert_eq!(
            session.console().front().map(|line| line.text.as_str()),
            Some("line 10"),
            "the oldest lines should have fallen off the front"
        );
        let before = session.console_revision();
        session.clear_console();
        assert!(session.console().is_empty());
        pretty_assertions::assert_ne!(session.console_revision(), before);
    }

    /// Clearing an already-empty transcript is not a change, so a floater
    /// watching the revision is not woken by a no-op.
    #[test]
    fn clearing_an_empty_console_is_not_a_change() {
        let mut session = RlvSession::default();
        let before = session.console_revision();
        session.clear_console();
        assert_eq!(session.console_revision(), before);
    }

    /// The restriction revision is separate from the console's, so echoing a
    /// command does not make the Restrictions floater rebuild.
    #[test]
    fn logging_does_not_bump_the_restriction_revision() {
        let mut session = RlvSession::default();
        let before = session.revision();
        session.log(RlvConsoleKind::Input, "@detach=n");
        assert_eq!(session.revision(), before);
        session.bump();
        pretty_assertions::assert_ne!(session.revision(), before);
    }

    /// With RLV off nothing is swallowed — an object's attempt to command the
    /// viewer is exactly what a person wants to see in that case.
    #[test]
    fn nothing_is_swallowed_while_rlv_is_off() {
        assert!(!swallows_owner_say(
            None,
            None,
            ChatSource::Object(ObjectKey::from(Uuid::from_u128(1))),
            ChatType::Owner,
            "@detach=n"
        ));
    }

    /// The two halves of the gate are both required, and neither is more than
    /// it says: an avatar saying `@detach=n` out loud is a person typing, and
    /// an object's ordinary chatter is conversation.
    ///
    /// Written against the roster's defaults rather than a store, so this pins
    /// the shape of the test and not a settings fixture; the enabled case is
    /// what [`crate::rlv`]'s consumers exercise with a real store.
    #[test]
    fn the_gate_needs_owner_say_and_the_prefix() {
        for (chat_type, message) in [
            (ChatType::Normal, "@detach=n"),
            (ChatType::Shout, "@detach=n"),
            (ChatType::Owner, "the collar is on"),
            (ChatType::Direct, "@detach=n"),
        ] {
            assert!(
                !swallows_owner_say(
                    None,
                    None,
                    ChatSource::Object(ObjectKey::from(Uuid::from_u128(1))),
                    chat_type,
                    message
                ),
                "{chat_type:?} {message:?} is not an RLV command line"
            );
        }
    }

    /// One `@notify` subscription and one command is a line the engine owes the
    /// grid, and it reaches the queue as the same [`sl_rlv::RlvReply`] a query
    /// answer does — which is the whole point of the single seam.
    #[test]
    fn a_notification_reaches_the_queue_as_a_reply() -> Result<(), Box<dyn core::error::Error>> {
        let mut session = RlvSession::default();
        let watcher = Uuid::from_u128(3);
        let collar = Uuid::from_u128(1);
        for (object, line) in [(watcher, "@notify:2222=n"), (collar, "@fly=n")] {
            let parsed = parse_chat_line(line).ok_or("not an RLV line")?;
            let command = parsed
                .first()
                .ok_or("no command")?
                .as_ref()
                .map_err(ToString::to_string)?;
            session.state_mut().apply(object, command);
        }
        assert!(session.state().has_notifications());
        let owed = session.state_mut().take_notifications();
        let notification = owed.last().ok_or("nothing owed")?.clone();
        session.push_reply(sl_rlv::RlvReply::from(notification));
        assert!(session.has_replies());
        let drained = session.take_replies();
        assert_eq!(drained.first().map(|reply| reply.channel), Some(2222));
        assert_eq!(
            drained.first().map(|reply| reply.message.as_str()),
            Some("/fly=n")
        );
        assert!(!session.has_replies());
        Ok(())
    }

    /// The reply queue is bounded like the transcript, and drains oldest-first
    /// so a script's answers arrive in the order it asked for them.
    #[test]
    fn the_reply_queue_is_a_bounded_ring() -> Result<(), Box<dyn core::error::Error>> {
        let mut session = RlvSession::default();
        let collar = Uuid::from_u128(1);
        let parsed = parse_chat_line("@version=2222").ok_or("not an RLV line")?;
        let command = parsed
            .first()
            .ok_or("no command")?
            .as_ref()
            .map_err(ToString::to_string)?;
        let base = session
            .state()
            .answer(collar, command, &RlvNoFacts::new(Uuid::nil()))
            .reply
            .ok_or("no reply")?;
        assert!(!session.has_replies());
        for index in 0..RLV_REPLY_CAPACITY + 5 {
            let mut reply = base.clone();
            // The channel is what tells the lines apart; the queue keeps the
            // newest and drops the oldest whatever they say.
            reply.channel = 2000_i32.saturating_add(i32::try_from(index).unwrap_or(0));
            session.push_reply(reply);
        }
        let drained = session.take_replies();
        assert_eq!(drained.len(), RLV_REPLY_CAPACITY);
        assert_eq!(
            drained.first().map(|reply| reply.channel),
            Some(2005),
            "the oldest replies should have fallen off the front"
        );
        assert!(!session.has_replies());
        Ok(())
    }

    /// Turning RLV off releases everything the engine held — the state the
    /// reference reaches by restarting, which is what its own switch demands.
    /// The console survives, because it is the record of what happened.
    #[test]
    fn releasing_everything_empties_the_engine() -> Result<(), Box<dyn core::error::Error>> {
        let mut session = RlvSession::default();
        let collar = Uuid::from_u128(1);
        for line in ["@notify:2222=n", "@fly=n", "@detach=n"] {
            let parsed = parse_chat_line(line).ok_or("not an RLV line")?;
            let command = parsed
                .first()
                .ok_or("no command")?
                .as_ref()
                .map_err(ToString::to_string)?;
            session.state_mut().apply(collar, command);
        }
        session.bump();
        session.log(RlvConsoleKind::Info, "a collar said something");
        let owed = session.state_mut().take_notifications();
        for notification in owed {
            session.push_reply(sl_rlv::RlvReply::from(notification));
        }
        assert!(session.has_replies());
        let before = session.revision();

        session.release_all();

        assert_eq!(session.state().restricting_objects().count(), 0);
        assert!(!session.state().has_behaviour(sl_rlv::RlvBehaviour::Fly));
        assert!(!session.state().has_notifications());
        assert!(
            !session.has_replies(),
            "a reply owed to a subscription that no longer exists must not go out"
        );
        pretty_assertions::assert_ne!(session.revision(), before);
        assert_eq!(session.console().len(), 1, "the log is not the state");
        Ok(())
    }

    /// The user's blocked keywords are their settings, not anything an object
    /// held, so a release carries them across. Without this, switching RLV off
    /// and on again would quietly hand `@setenv` back to the next device.
    #[test]
    fn a_release_keeps_the_blocked_keywords() {
        let mut session = RlvSession::default();
        assert!(session.state_mut().set_behaviour_blocked(
            "setenv",
            sl_rlv::RlvParamKind::AddRem,
            true
        ));
        session.release_all();
        assert!(
            session
                .state()
                .is_behaviour_blocked("setenv", sl_rlv::RlvParamKind::AddRem)
        );
        assert_eq!(session.state().blocked_behaviours().count(), 1);
    }

    /// Releasing an engine that held nothing is not a change, so a floater
    /// watching the revision is not woken by a switch flipped twice.
    #[test]
    fn releasing_nothing_is_not_a_change() {
        let mut session = RlvSession::default();
        let before = session.revision();
        session.release_all();
        assert_eq!(session.revision(), before);
    }

    /// A slot holding the reference viewer's own default sky.
    fn slot() -> RlvEnvironmentSlot {
        RlvEnvironmentSlot {
            rendered: Some(SkySettings::legacy_windlight_default("Default")),
            ..RlvEnvironmentSlot::default()
        }
    }

    /// With no environment ingested there is nothing to read and nothing to
    /// clone into the local layer, which is the one state a read cannot be
    /// answered from.
    #[test]
    fn an_empty_slot_answers_nothing() {
        let mut empty = RlvEnvironmentSlot::default();
        assert_eq!(empty.sky_value(RlvSkyField::Ambient), None);
        assert_eq!(empty.sky_direction(RlvSkyBody::Sun), None);
        assert!(!empty.set_sky_value(RlvSkyField::Gamma, RlvSkyValue::Float(2.0)));
        assert!(!empty.set_sky_angles(RlvSkyBody::Sun, 0.0, 0.0));
    }

    /// A write clones the rendered sky into the local layer rather than editing
    /// it, and every later read sees the edit — which is what lets one owner-say
    /// line write two components of the same colour.
    #[test]
    fn a_write_clones_the_rendered_sky_first() {
        let mut slot = slot();
        assert!(slot.set_sky_value(RlvSkyField::Gamma, RlvSkyValue::Float(2.5)));
        assert_eq!(
            slot.sky_value(RlvSkyField::Gamma),
            Some(RlvSkyValue::Float(2.5))
        );
        // The rendered sky — the scene's own — is untouched.
        assert_eq!(
            slot.rendered.as_ref().map(|sky| sky.gamma),
            Some(SkySettings::legacy_windlight_default("Default").gamma)
        );
        assert!(slot.edited.is_some());
    }

    /// Every field a subkey can name round-trips through the slot in the kind
    /// its declaration promises, so `sl-rlv` never has to guess.
    #[test]
    fn every_sky_field_round_trips() {
        let mut slot = slot();
        for field in [
            RlvSkyField::Ambient,
            RlvSkyField::BlueDensity,
            RlvSkyField::BlueHorizon,
            RlvSkyField::CloudColor,
            RlvSkyField::CloudPosDensity1,
            RlvSkyField::CloudPosDensity2,
            RlvSkyField::SunlightColor,
            RlvSkyField::Glow,
            RlvSkyField::CloudScrollRate,
            RlvSkyField::DensityMultiplier,
            RlvSkyField::DistanceMultiplier,
            RlvSkyField::DropletRadius,
            RlvSkyField::HazeDensity,
            RlvSkyField::HazeHorizon,
            RlvSkyField::IceLevel,
            RlvSkyField::MaxY,
            RlvSkyField::MoistureLevel,
            RlvSkyField::Gamma,
            RlvSkyField::CloudShadow,
            RlvSkyField::CloudScale,
            RlvSkyField::CloudVariance,
            RlvSkyField::MoonBrightness,
            RlvSkyField::MoonScale,
            RlvSkyField::SunScale,
            RlvSkyField::StarBrightness,
            RlvSkyField::CloudTexture,
            RlvSkyField::MoonTexture,
            RlvSkyField::SunTexture,
        ] {
            let written = match field.kind() {
                sl_rlv::RlvSkyKind::Float => RlvSkyValue::Float(0.375),
                sl_rlv::RlvSkyKind::Color => RlvSkyValue::Color([0.25, 0.5, 0.75]),
                sl_rlv::RlvSkyKind::Vec2 => RlvSkyValue::Vec2([0.25, 0.5]),
                sl_rlv::RlvSkyKind::Texture => RlvSkyValue::Texture(Uuid::from_u128(7)),
            };
            assert!(
                slot.set_sky_value(field, written),
                "{field:?} is not writable"
            );
            assert_eq!(slot.sky_value(field), Some(written), "{field:?}");
        }
    }

    /// A texture written as the null id names no texture, and reads back as the
    /// null id rather than as nothing — the reference's own answer for a sky
    /// that carries the viewer's default.
    #[test]
    fn the_null_texture_is_the_viewer_default() {
        let mut slot = slot();
        assert!(slot.set_sky_value(RlvSkyField::SunTexture, RlvSkyValue::Texture(Uuid::nil())));
        assert_eq!(
            slot.edited.as_ref().and_then(|sky| sky.sun_texture),
            None,
            "a null id is stored as no texture at all"
        );
        assert_eq!(
            slot.sky_value(RlvSkyField::SunTexture),
            Some(RlvSkyValue::Texture(Uuid::nil()))
        );
    }

    /// Placing the sun by angle and reading its direction back is a round trip:
    /// the two halves of the reference's own conversion.
    #[test]
    fn the_sun_round_trips_through_its_angles() {
        let mut slot = slot();
        assert!(slot.set_sky_angles(RlvSkyBody::Sun, 0.0, 0.0));
        let direction = slot.sky_direction(RlvSkyBody::Sun).unwrap_or([0.0; 3]);
        assert_eq!(
            direction.map(|part| format!("{part:.3}")),
            ["1.000".to_owned(), "0.000".to_owned(), "0.000".to_owned()],
            "a zero azimuth and elevation is due east on the horizon"
        );
    }

    /// A preset or day cycle named by **id** is the same request as
    /// `@setenv_asset`; a name is searched for in the library, and one the
    /// library does not carry is refused, which the script hears as a bad
    /// option.
    #[test]
    fn a_preset_is_resolved_by_id_first() {
        let mut slot = slot();
        let id = Uuid::from_u128(0x5eed);
        assert!(slot.apply_environment(&RlvEnvRequest::Preset(id.to_string())));
        assert_eq!(slot.request, Some(RlvEnvRequest::Asset(id)));
        assert!(!slot.apply_environment(&RlvEnvRequest::Preset("Sunrise".to_owned())));
        assert!(!slot.apply_environment(&RlvEnvRequest::DayCycle("Default".to_owned())));
        assert!(
            !slot.apply_environment(&RlvEnvRequest::Preset(Uuid::nil().to_string())),
            "a null id names no asset, and no library item is called that either"
        );
    }

    /// **A library name resolves once the index has published one**, and each
    /// command searches its own kind: `@setenv_preset` is `ST_SKY` and
    /// `@setenv_daycycle` is `ST_DAYCYCLE`, so a day cycle is not an answer to
    /// a preset even under the very same name.
    #[test]
    fn a_library_name_resolves_within_its_own_kind() {
        let mut slot = slot();
        let sky = Uuid::from_u128(0xA1);
        let day = Uuid::from_u128(0xA2);
        slot.library_environments
            .insert(SettingsKind::Sky, "Sunrise", sky);
        slot.library_environments
            .insert(SettingsKind::DayCycle, "Sunrise", day);

        assert!(slot.apply_environment(&RlvEnvRequest::Preset("Sunrise".to_owned())));
        assert_eq!(slot.request, Some(RlvEnvRequest::Asset(sky)));
        assert!(slot.apply_environment(&RlvEnvRequest::DayCycle("Sunrise".to_owned())));
        assert_eq!(slot.request, Some(RlvEnvRequest::Asset(day)));

        // `boost::iequals`: the name match is case-insensitive.
        assert!(slot.apply_environment(&RlvEnvRequest::Preset("sUNRISE".to_owned())));
        assert_eq!(slot.request, Some(RlvEnvRequest::Asset(sky)));

        // Water is indexed but no `@setenv_*` command searches it, so a water
        // preset of that name never becomes an answer to either command.
        slot.library_environments
            .insert(SettingsKind::Water, "Deep", Uuid::from_u128(0xA3));
        assert!(!slot.apply_environment(&RlvEnvRequest::Preset("Deep".to_owned())));
        assert!(!slot.apply_environment(&RlvEnvRequest::DayCycle("Deep".to_owned())));
    }

    /// A repeated library name keeps the **first** published, as the reference's
    /// `items.front()` does — a second item of that name never displaces it.
    #[test]
    fn a_repeated_library_name_keeps_the_first() {
        let mut library = RlvLibraryEnvironments::default();
        assert!(library.is_empty());
        library.insert(SettingsKind::Sky, "Sunrise", Uuid::from_u128(1));
        library.insert(SettingsKind::Sky, "SUNRISE", Uuid::from_u128(2));
        assert_eq!(library.len(), 1);
        assert_eq!(
            library.resolve(SettingsKind::Sky, "sunrise"),
            Some(Uuid::from_u128(1))
        );
    }

    /// A whole-environment change replaces the layer, so it takes the sky a
    /// script was editing with it — there is no layer left for that sky to be
    /// the top of, which is also where the reference ends up after installing
    /// the edit and then replacing it.
    #[test]
    fn a_whole_environment_change_drops_a_waiting_edit() {
        for request in [
            RlvEnvRequest::Clear,
            RlvEnvRequest::DayTime(0.5),
            RlvEnvRequest::Asset(Uuid::from_u128(9)),
        ] {
            let mut slot = slot();
            assert!(slot.set_sky_value(RlvSkyField::Gamma, RlvSkyValue::Float(2.5)));
            assert!(slot.edited.is_some());
            assert!(slot.apply_environment(&request));
            assert_eq!(slot.edited, None, "{request:?}");
            assert_eq!(slot.request, Some(request));
        }
    }
}
