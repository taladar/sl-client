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
//!   is something new to draw), and the **console transcript**: the lines the
//!   RLVa console shows.
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

use std::collections::VecDeque;

use bevy::prelude::*;
use sl_rlv::{RlvState, is_rlv_line};
use sl_settings::SettingValue;
use sl_viewer_settings::ViewerSettings;

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

/// The viewer's one RLV state machine, its revision, and the console
/// transcript.
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

    /// Empty the transcript (the console's Clear button).
    pub fn clear_console(&mut self) {
        if self.console.is_empty() {
            return;
        }
        self.console.clear();
        self.console_revision = self.console_revision.wrapping_add(1);
    }
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

#[cfg(test)]
mod tests {
    use super::{
        RLV_BOOL_SETTINGS, RLV_CONSOLE_CAPACITY, RLV_PREFIX_SETTINGS, RLV_STRINGS, RlvConsoleKind,
        RlvSession, rlv_flag, rlv_string, rlv_string_def,
    };
    use pretty_assertions::assert_eq;
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
}
