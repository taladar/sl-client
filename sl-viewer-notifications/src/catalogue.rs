//! **The notification catalogue**, one module per family.
//!
//! The catalogue is ~1,300 declarative entries — too many to read, edit or
//! review in one file, and they fall naturally into the families the
//! `viewer-notification-catalogue-*` tasks ported. Each family is a module
//! holding its own `ENTRIES` slice; this module joins them, in source order,
//! into the single [`NOTIFICATIONS`] slice the host reads.
//!
//! The join happens at **compile time**: [`NOTIFICATIONS`] is still one
//! `&'static [NotificationTemplate]`, so nothing downstream pays for the
//! split and the catalogue stays usable from a `const` context.

mod appearance_wearables;
mod avatar_movement;
mod confirmations;
mod diagnostics;
mod estate_region;
mod experiences;
mod friends_people;
mod generic;
mod groups;
mod im_chat;
mod info_tips;
mod inventory;
mod land_parcel;
mod landmarks_navigation;
mod login_session;
mod marketplace;
mod media_sound;
mod misc;
mod money_economy;
mod objects_edit;
mod preferences;
mod premium_account;
mod rlv;
mod scripts;
mod security;
mod server_alerts;
mod snapshot_social;
mod teleport;
mod ui_hints;
mod voice;
mod web_browser;

use crate::NotificationTemplate;

/// Every family's entries, in the order they appear in [`NOTIFICATIONS`].
///
/// Adding a family is a module, an entry here, and nothing else: the length
/// and the flattening below both derive from this list.
const FAMILIES: &[&[NotificationTemplate]] = &[
    generic::ENTRIES,
    server_alerts::ENTRIES,
    confirmations::ENTRIES,
    info_tips::ENTRIES,
    appearance_wearables::ENTRIES,
    avatar_movement::ENTRIES,
    diagnostics::ENTRIES,
    estate_region::ENTRIES,
    ui_hints::ENTRIES,
    misc::ENTRIES,
    login_session::ENTRIES,
    marketplace::ENTRIES,
    snapshot_social::ENTRIES,
    objects_edit::ENTRIES,
    im_chat::ENTRIES,
    preferences::ENTRIES,
    friends_people::ENTRIES,
    groups::ENTRIES,
    land_parcel::ENTRIES,
    media_sound::ENTRIES,
    money_economy::ENTRIES,
    landmarks_navigation::ENTRIES,
    inventory::ENTRIES,
    scripts::ENTRIES,
    web_browser::ENTRIES,
    security::ENTRIES,
    teleport::ENTRIES,
    premium_account::ENTRIES,
    voice::ENTRIES,
    experiences::ENTRIES,
    rlv::ENTRIES,
];

/// The total number of catalogue entries across every family — the length of
/// the flattened [`NOTIFICATIONS`] array.
#[expect(
    clippy::indexing_slicing,
    reason = "a const fn cannot iterate; the index is bounded by the loop condition and any error is a compile-time failure, not a runtime panic"
)]
const fn catalogue_len() -> usize {
    let mut total = 0_usize;
    let mut family = 0_usize;
    while family < FAMILIES.len() {
        total = total.saturating_add(FAMILIES[family].len());
        family = family.saturating_add(1);
    }
    total
}

/// The length of [`NOTIFICATIONS`], computed from [`FAMILIES`].
const CATALOGUE_LEN: usize = catalogue_len();

/// Copy every family's entries into one array, so [`NOTIFICATIONS`] is a
/// single flat slice rather than a slice of slices. Rust has no way to
/// concatenate `const` slices, so the copy is written out; it runs during
/// const evaluation and costs nothing at run time.
#[expect(
    clippy::indexing_slicing,
    reason = "a const fn cannot iterate; every index is bounded by its loop condition and any error is a compile-time failure, not a runtime panic"
)]
const fn flatten_families() -> [NotificationTemplate; CATALOGUE_LEN] {
    // Every slot is overwritten below; an array of a type with no `const`
    // default has to start from some value, so it starts from the first entry.
    let mut flat = [FAMILIES[0][0]; CATALOGUE_LEN];
    let mut family = 0_usize;
    let mut written = 0_usize;
    while family < FAMILIES.len() {
        let entries = FAMILIES[family];
        let mut index = 0_usize;
        while index < entries.len() {
            flat[written] = entries[index];
            written = written.saturating_add(1);
            index = index.saturating_add(1);
        }
        family = family.saturating_add(1);
    }
    flat
}

/// **The catalogue.** Every notification the host can raise, declared as data.
///
/// A curated port of the reference's `notifications.xml` — not its full ~1,300
/// entries (each bespoke dialog owns its own form), but the notifications with
/// **no dialog of their own** that today fall back to the generic raw-string
/// `SystemMessage` / `GenericAlert`:
///
/// - The generic fallbacks and demo exemplars first (`SystemTip` …
///   `ConfirmQuit`) — one of each kind, exercising `[KEY]` substitution, the
///   `unique` dedup and the ignore checkbox.
/// - **Keyed server alerts** the simulator sends by `AlertInfo` key
///   (`notification_host::ingest_alert_messages` raises these when the
///   key matches): the maturity / access-blocked family, the region-restart
///   countdowns, and standalone failure notices.
/// - **Standard action-confirmation modals** shared across features (empty
///   trash, remove friend, leave group, logged-out, agree-to-login) — raised by
///   their owning feature, but the *entry* (text, buttons) belongs here.
/// - **Info tips / notifies** not routed to nearby chat (landmark created,
///   granted-modify-rights, a help tip).
/// - **The appearance & wearables family**
///   (`viewer-notification-catalogue-appearance-wearables`): outfit / wearable
///   editing confirms, attachment prompts, the avatar-rez diagnostics tips and
///   the server-keyed attach / drop refusals.
/// - **The avatar movement family**
///   (`viewer-notification-catalogue-avatar-movement`): animation upload / AO
///   set management, movement-mode toggle tips and the server-keyed sit /
///   stand refusals.
/// - **The diagnostics family**
///   (`viewer-notification-catalogue-diagnostics`): installation / hardware
///   warnings, file-handling failures and the local-file watcher errors.
/// - **The estate & region management family**
///   (`viewer-notification-catalogue-estate-region`): region tools, terrain
///   validation, estate access lists / scope choosers, admin kick / freeze
///   prompts, pathfinding state and the server-keyed freeze / eject / entry
///   refusals.
/// - **The consolidated remainder** (one section per
///   `viewer-notification-catalogue-<family>` task): experiences,
///   friends / people, groups, IM / chat, inventory, land / parcel,
///   landmarks / navigation, login / session, marketplace, media / sound,
///   misc, money / economy, objects / edit, preferences, premium account,
///   RLV, scripts, security, snapshot / social, teleport, UI hints, voice
///   and web browser.
///
/// See `viewer-notification-catalogue`.
pub const NOTIFICATIONS: &[NotificationTemplate] = &flatten_families();
