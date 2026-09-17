//! Keyed server alerts (raised by `ingest_alert_messages` when the
//! simulator's `AlertInfo` key names one of these).
//!
//! The maturity / access-blocked family: the simulator blocks an entry, a
//! land claim or a land buy whose maturity rating exceeds the agent's
//! preference. The reference's `_Change` / `_AdultsOnlyContent` variants are
//! deferred (they carry a "change my preference and retry" callback that
//! needs the maturity-preference plumbing); this ports the plain refusals.
//!
//! One family of [the catalogue](crate::NOTIFICATIONS), joined into it
//! in source order by `catalogue`.

use crate::{
    NO_FORM, NotificationIgnore, NotificationKind, NotificationPriority, NotificationTemplate,
    OK_FORM,
};

/// The keyed server alerts family's catalogue entries.
pub(crate) const ENTRIES: &[NotificationTemplate] = &[
    NotificationTemplate {
        name: "RegionEntryAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-region-entry-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TeleportEntryAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-teleport-entry-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LandClaimAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-land-claim-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LandBuyAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-land-buy-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    // The non-modal maturity notice the reference shows on a soft block (a tip
    // that logs to chat), with the `[REGIONMATURITY]` substitution.
    NotificationTemplate {
        name: "RegionEntryAccessBlocked_Notify",
        kind: NotificationKind::Tip,
        message_key: "notification-region-entry-access-blocked-notify",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    // The seconds-granularity restart countdown — the companion to the existing
    // `RegionRestartMinutes`, with `[NAME]` / `[SECONDS]`.
    NotificationTemplate {
        name: "RegionRestartSeconds",
        kind: NotificationKind::Alert,
        message_key: "notification-region-restart-seconds",
        title_key: None,
        priority: NotificationPriority::High,
        persist: true,
        log_to_chat: true,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    // Standalone failure notices the simulator sends by key.
    NotificationTemplate {
        name: "TooManyScripts",
        kind: NotificationKind::Notify,
        message_key: "notification-too-many-scripts",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FailedToPlaceObject",
        kind: NotificationKind::Notify,
        message_key: "notification-failed-to-place-object",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FailedToFindWearableUnnamed",
        kind: NotificationKind::Notify,
        message_key: "notification-failed-to-find-wearable",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "HomePositionSet",
        kind: NotificationKind::Notify,
        message_key: "notification-home-position-set",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
];
