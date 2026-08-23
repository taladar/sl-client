//! Composed avatar name-tag **content** — the lines a tag shows and their
//! colours (the reference viewer's `LLVOAvatar::idleUpdateNameTagText`).
//!
//! This module owns the *what* of a tag; the *how* (world-space billboard
//! rendering) lives in [`crate::name_tag_billboard`]. The renderer consumes a
//! [`TagContent`] component on each tag entity and rebuilds text layout and
//! mesh only when the composed value actually changes, so composition here is
//! deliberately change-driven: assemble, compare, and only then assign.
//!
//! # The render-cost lines
//!
//! The tag is also where an avatar's render cost (ARC) surfaces
//! (`viewer-name-tags-complexity-distance`), measured by
//! [`crate::avatar_complexity`]. Two lines, both the reference's:
//!
//! - **`Complexity: N`**, coloured on a green→amber→red ramp of the cost
//!   against the complexity budget — green at nothing, amber at exactly the
//!   budget, saturating red at twice it. With no budget set there is nothing to
//!   judge against, so the number is reported in neutral grey instead; the same
//!   goes for your own tag, where a red rating would be telling you off for a
//!   limit that does not apply to you.
//! - **`Texture Area: N m²`**, in red, only when an avatar's attachments cover
//!   more than the area limit. It appears alongside the cost rather than
//!   instead of it: the reference notes that untangling *which* limit fired
//!   would cost more than it explains, and shows the cost either way.
//!
//! Three settings gate them, with the reference's defaults, and they compose to
//! a quiet default: the cost appears only on avatars the limiter is actually
//! doing something about ([`SETTING_SHOW_COMPLEXITY`],
//! [`SETTING_SHOW_COMPLEXITY_WHEN_LIMITED_ONLY`]), and your own cost — which
//! the radar cannot show, since it lists *other* avatars — only once you ask
//! for it ([`SETTING_SHOW_OWN_COMPLEXITY`]).

use bevy::prelude::*;
use std::collections::HashSet;

use sl_client_bevy::{AgentKey, SlEvent, SlSessionEvent};

/// Show display names on tags (the reference `NameTagShowDisplayNames`,
/// default on). Off = legacy names only.
pub(crate) const SETTING_SHOW_DISPLAY_NAMES: &str = "ShowDisplayNames";

/// Show the small `first.last` username line under a custom display name
/// (the reference `NameTagShowUsernames`, default on).
pub(crate) const SETTING_SHOW_USERNAMES: &str = "ShowUsernames";

/// Show the group-title line (the reference `NameTagShowGroupTitles`,
/// default on).
pub(crate) const SETTING_SHOW_GROUP_TITLES: &str = "ShowGroupTitles";

/// Colour friends' tags (the reference `NameTagShowFriends`, default on).
pub(crate) const SETTING_SHOW_FRIEND_COLOR: &str = "ShowFriendColor";

/// Show the avatar-distance line (Firestorm `FSTagShowDistance`; FS defaults
/// it off, but this viewer ships it on — the user asked for it and no
/// preferences UI exposes the toggle yet). The distance is measured from the
/// **own avatar** (like the reference); only the fade/cut-off is camera-based.
pub(crate) const SETTING_SHOW_DISTANCE: &str = "ShowDistance";

/// Show the Typing status (Firestorm `FSShowTypingStateInNameTag`; same
/// on-by-default deviation as [`SETTING_SHOW_DISTANCE`]).
pub(crate) const SETTING_SHOW_TYPING: &str = "ShowTyping";

/// Show the `Auto-Response` status on the **own** tag while an autorespond
/// mode is on (Firestorm `FSShowAutorespondInNametag`, default off — the state
/// is yours alone, and the reference leaves it hidden until asked for).
pub(crate) const SETTING_SHOW_AUTORESPONSE: &str = "ShowAutorespondInNameTag";

/// Tint the whole tag by chat-range band (Firestorm
/// `FSTagShowDistanceColors`, default off).
pub(crate) const SETTING_COLOR_BY_DISTANCE: &str = "ColorByDistance";

/// Show the render-cost (ARC) line at all (Firestorm `FSTagShowARW`, default
/// on) — the master switch over the two that follow. On its own it shows
/// nothing: [`SETTING_SHOW_COMPLEXITY_WHEN_LIMITED_ONLY`] is also on by
/// default, so out of the box the number appears only on an avatar the
/// complexity limit is actually doing something about.
pub(crate) const SETTING_SHOW_COMPLEXITY: &str = "ShowComplexity";

/// Show the render-cost line on your **own** tag (Firestorm `FSTagShowOwnARW`,
/// default off). This is the only read-out of your own ARC — the radar lists
/// nearby avatars and excludes you, exactly as the reference radar does — so
/// it is what you turn on to find out whether *you* are the expensive one.
pub(crate) const SETTING_SHOW_OWN_COMPLEXITY: &str = "ShowOwnComplexity";

/// Show other avatars' render cost **only** when the viewer is limiting them
/// (Firestorm `FSTagShowTooComplexOnlyARW`, default on). Off shows it on every
/// avatar — informative, and a great deal of text over a crowd.
pub(crate) const SETTING_SHOW_COMPLEXITY_WHEN_LIMITED_ONLY: &str = "ShowComplexityWhenLimitedOnly";

/// The font size, physical px at scale factor 1, of the main name line (the
/// reference renders the name in `SansSerif`; the tag previously used 16 px).
pub(crate) const NAME_FONT_SIZE_PX: f32 = 16.0;

/// The font size of the auxiliary lines — status, group title, username,
/// distance (the reference's `SansSerifSmall`; small/medium ratio 0.8 → 13 px
/// against the 16 px name line).
pub(crate) const SMALL_FONT_SIZE_PX: f32 = 13.0;

/// The relative font tier of one tag line; the renderer maps tiers to sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagLineSize {
    /// The main name line (reference `SansSerif`).
    Name,
    /// An auxiliary line (reference `SansSerifSmall`).
    Small,
}

impl TagLineSize {
    /// The font size, in logical px, this tier renders at.
    pub(crate) const fn font_size_px(self) -> f32 {
        match self {
            Self::Name => NAME_FONT_SIZE_PX,
            Self::Small => SMALL_FONT_SIZE_PX,
        }
    }
}

/// One composed line of a name tag, top-to-bottom order in
/// [`TagContent::lines`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TagLine {
    /// The line's text (no trailing newline; the renderer joins lines).
    pub(crate) text: String,
    /// The line's font tier.
    pub(crate) size: TagLineSize,
    /// The line's colour.
    pub(crate) color: Color,
}

/// The composed content of one avatar's tag; a component on the tag (label)
/// entity. The renderer rebuilds spans/layout/mesh on `Changed<TagContent>`,
/// so writers must compare before assigning.
#[derive(Component, Debug, Clone, PartialEq, Default)]
pub(crate) struct TagContent {
    /// Ordered top-to-bottom: `[status?, group title?, name, username?,
    /// distance?]`.
    pub(crate) lines: Vec<TagLine>,
    /// The resolved whole-tag colour (the name/status/title line tint; the
    /// bubble itself stays the reference's black backdrop regardless).
    pub(crate) base_color: Color,
}

impl TagContent {
    /// A single plain white name line — the minimal tag shown until the
    /// composer has resolved richer content.
    pub(crate) fn plain_name(name: impl Into<String>) -> Self {
        Self {
            lines: vec![TagLine {
                text: name.into(),
                size: TagLineSize::Name,
                color: Color::WHITE,
            }],
            base_color: Color::WHITE,
        }
    }
}

// ---------------------------------------------------------------------------
// Colours (values from the reference's colors.xml).
// ---------------------------------------------------------------------------

/// Base tag colour: display names off (`NameTagLegacy`) and a *default*
/// display name (`NameTagMatch`) — both White in the reference, one
/// [`TagColors::default`] slot here.
const NAME_TAG_LEGACY: Color = Color::WHITE;

/// Base tag colour for a *custom* display name (`NameTagMismatch`, LtGray).
const NAME_TAG_MISMATCH: Color = Color::srgb(0.9, 0.9, 0.9);

/// The own tag's colour (`NameTagSelf` via the reference's contact-set
/// colouring — ships as plain White, so self reads as the default; the
/// precedence slot exists so a themed override stays a one-line change).
const NAME_TAG_SELF: Color = Color::WHITE;

/// A friend's tag colour (`NameTagFriend`), gated on
/// [`SETTING_SHOW_FRIEND_COLOR`].
const NAME_TAG_FRIEND: Color = Color::srgb(0.75, 0.92, 0.49);

/// A muted avatar's tag colour (`NameTagMuted`).
const NAME_TAG_MUTED: Color = Color::srgb(0.4, 0.4, 0.4);

/// A Linden (grid staff) tag colour (`NameTagLinden`, LtBlue).
const NAME_TAG_LINDEN: Color = Color::srgb(0.0, 0.5, 1.0);

/// Distance-line colour inside whisper range (`NameTagWhisperDistanceColor`).
const DISTANCE_WHISPER_COLOR: Color = Color::srgb(0.0, 1.0, 0.0);

/// Distance-line colour inside normal chat range
/// (`NameTagChatDistanceColor`).
const DISTANCE_CHAT_COLOR: Color = Color::srgb(0.0, 1.0, 0.0);

/// Distance-line colour inside shout range (`NameTagShoutDistanceColor`).
const DISTANCE_SHOUT_COLOR: Color = Color::srgb(1.0, 1.0, 0.0);

/// Distance-line colour beyond shout range
/// (`NameTagBeyondShoutDistanceColor`, BrightRed).
const DISTANCE_BEYOND_COLOR: Color = Color::srgb(1.0, 0.0, 0.0);

/// Whisper radius, metres.
const WHISPER_RANGE_METRES: f32 = 10.0;

/// Normal chat radius, metres.
const CHAT_RANGE_METRES: f32 = 20.0;

/// Shout radius, metres.
const SHOUT_RANGE_METRES: f32 = 100.0;

/// The render-cost line's colour when no budget is set, and on your own tag —
/// nothing is being judged, so the number is reported rather than rated. The
/// reference's `grey1`, hardcoded there as here (unlike the distance bands,
/// these two are not `colors.xml` entries, so they are not skin tokens).
const COMPLEXITY_UNRATED_COLOR: Color = Color::srgb(0.8, 0.8, 0.8);

/// The attachment-surface-area line's colour (the reference's plain red): it
/// only ever appears when the area is over the limit, so it is always a
/// complaint.
const TEXTURE_AREA_COLOR: Color = Color::srgb(1.0, 0.0, 0.0);

/// The reference desaturates the username line to 83% of the name colour
/// (`llvoavatar.cpp`).
const USERNAME_DESATURATE: f32 = 0.83;

/// Which nearby avatars are currently typing, from the `ChatTyping` start/stop
/// chat messages — deliberately the **state signal**, not the optional TYPE
/// animation (many viewers disable sending the animation, but the start/stop
/// messages are what drives it in the first place). Decoupled from the
/// conversations model, whose per-session typing map has open-window
/// semantics that don't fit a world tag.
#[derive(Resource, Debug, Default)]
pub(crate) struct NameTagStatuses {
    /// Agents whose last typing signal was "start".
    chat_typing: HashSet<AgentKey>,
}

impl NameTagStatuses {
    /// Whether `agent` is currently typing in nearby chat.
    pub(crate) fn is_typing(&self, agent: AgentKey) -> bool {
        self.chat_typing.contains(&agent)
    }
}

/// Fold `ChatTyping` start/stop signals into [`NameTagStatuses`] (writing the
/// resource only when membership actually changes, so its change tick stays
/// meaningful).
pub(crate) fn ingest_tag_statuses(
    mut events: MessageReader<SlEvent>,
    mut statuses: ResMut<NameTagStatuses>,
) {
    for event in events.read() {
        if let SlSessionEvent::ChatTyping {
            source_id, typing, ..
        } = &event.0
        {
            let agent = AgentKey::from(*source_id);
            // The membership pre-check is load-bearing: `contains` goes
            // through the shared `Deref`, so a no-op signal never flags the
            // `ResMut` as changed (insert/remove would, regardless of effect).
            let is_member = statuses.chat_typing.contains(&agent);
            if *typing && !is_member {
                statuses.chat_typing.insert(agent);
            } else if !*typing && is_member {
                statuses.chat_typing.remove(&agent);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Composition.
// ---------------------------------------------------------------------------

/// Everything the composer knows about one avatar when assembling its tag.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent avatar state feeding one line or colour of \
              the tag; folding them into enums would just obscure the mapping"
)]
#[derive(Debug, Default)]
pub(crate) struct TagInputs<'a> {
    /// Whether this is the logged-in avatar's own tag.
    pub(crate) is_self: bool,
    /// The avatar's resolved names, if any source answered yet.
    pub(crate) record: Option<&'a crate::avatars::NameRecord>,
    /// The id-fragment fallback shown until a name resolves.
    pub(crate) provisional: String,
    /// The avatar's group title, if any.
    pub(crate) title: Option<&'a str>,
    /// Whether the avatar is on the friends list.
    pub(crate) is_friend: bool,
    /// Whether the avatar is muted.
    pub(crate) is_muted: bool,
    /// Whether the avatar is typing in nearby chat.
    pub(crate) is_typing: bool,
    /// Whether the avatar's signalled animation set carries the AWAY entry.
    pub(crate) is_away: bool,
    /// Whether the avatar's signalled animation set carries the DO NOT DISTURB
    /// entry, shown as the reference's `Unavailable` status.
    pub(crate) is_do_not_disturb: bool,
    /// Whether *our own* tag should carry the `Auto-Response` status — one of
    /// the two autorespond modes is on ([`crate::presence`]). Purely local:
    /// autorespond has no wire representation, so it can only ever be true on
    /// the own tag.
    pub(crate) is_autoresponse: bool,
    /// Whether the avatar is editing its appearance (the CUSTOMIZE signalled
    /// animation), shown as the reference's `(Editing Appearance)` status.
    pub(crate) is_editing_appearance: bool,
    /// Own-avatar→avatar distance, metres; `None` suppresses the distance
    /// line (the own tag, or the own avatar not being placed yet).
    pub(crate) distance_m: Option<f32>,
    /// The avatar's measured render cost (ARC), or `None` while it has not been
    /// scored — which suppresses the complexity line entirely rather than
    /// showing a zero that means "not known yet".
    pub(crate) complexity: Option<u32>,
    /// The avatar's total attachment surface area, in square metres, feeding
    /// the texture-area line.
    pub(crate) attachment_area_m2: f32,
    /// Whether the viewer is currently limiting this avatar (drawing them as a
    /// jellydoll) — what the "only when limited" mode keys off.
    pub(crate) is_limited: bool,
}

/// The name-tag content toggles, resolved from the settings store once per
/// composer run.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a direct mirror of the independent boolean settings the tag honours"
)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct TagToggles {
    /// [`SETTING_SHOW_DISPLAY_NAMES`].
    pub(crate) show_display_names: bool,
    /// [`SETTING_SHOW_USERNAMES`].
    pub(crate) show_usernames: bool,
    /// [`SETTING_SHOW_GROUP_TITLES`].
    pub(crate) show_group_titles: bool,
    /// [`SETTING_SHOW_FRIEND_COLOR`].
    pub(crate) show_friend_color: bool,
    /// [`SETTING_SHOW_DISTANCE`].
    pub(crate) show_distance: bool,
    /// [`SETTING_SHOW_TYPING`].
    pub(crate) show_typing: bool,
    /// [`SETTING_SHOW_AUTORESPONSE`].
    pub(crate) show_autoresponse: bool,
    /// [`SETTING_COLOR_BY_DISTANCE`].
    pub(crate) color_by_distance: bool,
    /// [`SETTING_SHOW_COMPLEXITY`].
    pub(crate) show_complexity: bool,
    /// [`SETTING_SHOW_OWN_COMPLEXITY`].
    pub(crate) show_own_complexity: bool,
    /// [`SETTING_SHOW_COMPLEXITY_WHEN_LIMITED_ONLY`].
    pub(crate) complexity_when_limited_only: bool,
    /// The complexity budget the render-cost line is rated against
    /// (`crate::avatar_complexity`'s `RenderAvatarMaxComplexity`); `0` = no
    /// budget, so the number is reported unrated.
    pub(crate) complexity_limit: u32,
    /// The attachment-area limit, in square metres, past which the texture-area
    /// line appears; `0` = no limit, so it never does.
    pub(crate) area_limit_m2: f32,
}

impl Default for TagToggles {
    fn default() -> Self {
        Self {
            show_display_names: true,
            show_usernames: true,
            show_group_titles: true,
            show_friend_color: true,
            show_distance: true,
            show_typing: true,
            show_autoresponse: false,
            color_by_distance: false,
            show_complexity: true,
            show_own_complexity: false,
            complexity_when_limited_only: true,
            complexity_limit: 0,
            area_limit_m2: 0.0,
        }
    }
}

impl TagToggles {
    /// Resolve the toggles from the settings store (all default-on except the
    /// distance tint, matching the registered defaults).
    fn from_settings(settings: Option<&crate::settings::ViewerSettings>) -> Self {
        let Some(settings) = settings else {
            return Self::default();
        };
        let store = settings.store();
        let get = |name: &str, default: bool| store.get_bool(name).unwrap_or(default);
        Self {
            show_display_names: get(SETTING_SHOW_DISPLAY_NAMES, true),
            show_usernames: get(SETTING_SHOW_USERNAMES, true),
            show_group_titles: get(SETTING_SHOW_GROUP_TITLES, true),
            show_friend_color: get(SETTING_SHOW_FRIEND_COLOR, true),
            show_distance: get(SETTING_SHOW_DISTANCE, true),
            show_typing: get(SETTING_SHOW_TYPING, true),
            show_autoresponse: get(SETTING_SHOW_AUTORESPONSE, false),
            color_by_distance: get(SETTING_COLOR_BY_DISTANCE, false),
            show_complexity: get(SETTING_SHOW_COMPLEXITY, true),
            show_own_complexity: get(SETTING_SHOW_OWN_COMPLEXITY, false),
            complexity_when_limited_only: get(SETTING_SHOW_COMPLEXITY_WHEN_LIMITED_ONLY, true),
            // The limits themselves belong to the complexity limiter; the tag
            // only rates its number against them.
            complexity_limit: store
                .get_u32(crate::avatar_complexity::SETTING_MAX_COMPLEXITY)
                .unwrap_or(0),
            area_limit_m2: store
                .get_f32(crate::avatar_complexity::SETTING_SURFACE_AREA_LIMIT)
                .unwrap_or(0.0),
        }
    }
}

/// The name-tag colour palette, resolved once per composer run from the
/// settings store — the active skin's tokens under any per-account overrides
/// (the [`crate::skin_colors`] bridge, edited on the preferences colors &
/// skins tab). [`Default`] is the built-in palette (the reference
/// `colors.xml` values), used by tests and settings-less apps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TagColors {
    /// The base tag colour: legacy names and matching display names
    /// (`NameTagLegacy` / `NameTagMatch`).
    pub(crate) default: Color,
    /// The own tag's colour (`NameTagSelf`).
    pub(crate) self_: Color,
    /// A friend's tag colour (`NameTagFriend`), gated on
    /// [`TagToggles::show_friend_color`].
    pub(crate) friend: Color,
    /// A muted avatar's tag colour (`NameTagMuted`).
    pub(crate) muted: Color,
    /// A Linden (grid staff) tag colour (`NameTagLinden`).
    pub(crate) linden: Color,
    /// A custom display name's tag colour (`NameTagMismatch`).
    pub(crate) mismatch: Color,
    /// Distance-band colour inside whisper range.
    pub(crate) distance_whisper: Color,
    /// Distance-band colour inside normal chat range.
    pub(crate) distance_chat: Color,
    /// Distance-band colour inside shout range.
    pub(crate) distance_shout: Color,
    /// Distance-band colour beyond shout range.
    pub(crate) distance_beyond: Color,
}

impl Default for TagColors {
    fn default() -> Self {
        Self {
            default: NAME_TAG_LEGACY,
            self_: NAME_TAG_SELF,
            friend: NAME_TAG_FRIEND,
            muted: NAME_TAG_MUTED,
            linden: NAME_TAG_LINDEN,
            mismatch: NAME_TAG_MISMATCH,
            distance_whisper: DISTANCE_WHISPER_COLOR,
            distance_chat: DISTANCE_CHAT_COLOR,
            distance_shout: DISTANCE_SHOUT_COLOR,
            distance_beyond: DISTANCE_BEYOND_COLOR,
        }
    }
}

impl TagColors {
    /// Resolve the palette from the settings store; without one the built-in
    /// [`Default`] palette stands.
    fn from_settings(settings: Option<&crate::settings::ViewerSettings>) -> Self {
        let color = |name: &str| crate::skin_colors::setting_color(settings, name);
        Self {
            default: color(crate::skin_colors::SETTING_NAME_TAG_DEFAULT),
            self_: color(crate::skin_colors::SETTING_NAME_TAG_SELF),
            friend: color(crate::skin_colors::SETTING_NAME_TAG_FRIEND),
            muted: color(crate::skin_colors::SETTING_NAME_TAG_MUTED),
            linden: color(crate::skin_colors::SETTING_NAME_TAG_LINDEN),
            mismatch: color(crate::skin_colors::SETTING_NAME_TAG_MISMATCH),
            distance_whisper: color(crate::skin_colors::SETTING_NAME_TAG_DISTANCE_WHISPER),
            distance_chat: color(crate::skin_colors::SETTING_NAME_TAG_DISTANCE_CHAT),
            distance_shout: color(crate::skin_colors::SETTING_NAME_TAG_DISTANCE_SHOUT),
            distance_beyond: color(crate::skin_colors::SETTING_NAME_TAG_DISTANCE_BEYOND),
        }
    }
}

/// The chat-range band colour for a distance (the reference's
/// whisper / say / shout / beyond bands), from the resolved palette.
fn distance_band_color(distance_m: f32, colors: &TagColors) -> Color {
    if distance_m <= WHISPER_RANGE_METRES {
        colors.distance_whisper
    } else if distance_m <= CHAT_RANGE_METRES {
        colors.distance_chat
    } else if distance_m <= SHOUT_RANGE_METRES {
        colors.distance_shout
    } else {
        colors.distance_beyond
    }
}

/// Whether an avatar's tag carries the render-cost line, under the reference's
/// three-setting rule: the master switch, then your own tag by its own opt-in,
/// and everyone else either always or only while the viewer is limiting them.
pub(crate) const fn shows_complexity(
    is_self: bool,
    is_limited: bool,
    toggles: &TagToggles,
) -> bool {
    if !toggles.show_complexity {
        return false;
    }
    if is_self {
        return toggles.show_own_complexity;
    }
    !toggles.complexity_when_limited_only || is_limited
}

/// The render-cost line's colour: a green→amber→red ramp of the cost against
/// the budget (green at nothing, amber at exactly the budget, saturating red at
/// twice it), or the unrated grey when there is no budget to judge against.
///
/// The reference's ramp, unchanged: `green = 1 - clamp((c - max)/max, 0, 1)` and
/// `red = min(c/max, 1)`.
fn complexity_color(complexity: u32, limit: u32) -> Color {
    if limit == 0 {
        return COMPLEXITY_UNRATED_COLOR;
    }
    let (cost, limit) = (complexity_as_f32(complexity), complexity_as_f32(limit));
    let green = 1.0 - ((cost - limit) / limit).clamp(0.0, 1.0);
    let red = (cost / limit).min(1.0);
    Color::srgb(red, green, 0.0)
}

/// A cost or budget as `f32` for the ramp; both are far below the precision
/// threshold that would matter to a colour.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the value only picks a colour on a continuous ramp"
)]
const fn complexity_as_f32(value: u32) -> f32 {
    value as f32
}

/// Multiply a colour's RGB by a factor, keeping alpha (the reference's
/// username-line desaturation).
fn scale_rgb(color: Color, factor: f32) -> Color {
    let mut linear = color.to_linear();
    linear.red *= factor;
    linear.green *= factor;
    linear.blue *= factor;
    Color::LinearRgba(linear)
}

/// Whether a legacy name marks grid staff (the reference colours `* Linden`
/// tags `NameTagLinden`).
fn is_linden(record: Option<&crate::avatars::NameRecord>) -> bool {
    record
        .and_then(|record| record.legacy.as_deref())
        .is_some_and(|legacy| legacy.ends_with(" Linden"))
}

/// Assemble one avatar's tag content — the reference's
/// `idleUpdateNameTagText` line order and `getNameTagColor` precedence, pure
/// and unit-testable. Line order, top to bottom: status (comma-joined
/// `Away` / `Unavailable` / `Auto-Response` / `Blocked` / `Typing`), group
/// title, name, username, distance.
pub(crate) fn compose_tag(
    inputs: &TagInputs<'_>,
    toggles: &TagToggles,
    colors: &TagColors,
) -> TagContent {
    // --- The whole-tag colour (the reference's precedence chain). ---
    // A user-given alias counts as a custom name too: the shown name is not
    // this person's legacy name, so it gets the mismatch colour and the
    // username line under it (`viewer-contact-set-pseudonyms`).
    let has_custom_display_name = toggles.show_display_names
        && inputs
            .record
            .is_some_and(crate::avatars::NameRecord::has_custom_display_name);
    // A legacy name and a matching display name share the base colour (the
    // reference's NameTagLegacy / NameTagMatch, both White).
    let display_base = if has_custom_display_name {
        colors.mismatch
    } else {
        colors.default
    };
    let base_color = if inputs.is_self {
        colors.self_
    } else if inputs.is_friend && toggles.show_friend_color {
        colors.friend
    } else if inputs.is_muted {
        colors.muted
    } else if is_linden(inputs.record) {
        colors.linden
    } else if toggles.color_by_distance {
        // The whole-tag distance tint applies only when no identity colour
        // (self / friend) claimed the tag.
        inputs.distance_m.map_or(display_base, |distance| {
            distance_band_color(distance, colors)
        })
    } else {
        display_base
    };

    let mut lines = Vec::new();

    // --- Status line (small, tag colour): Away, Blocked, Typing. ---
    let mut states: Vec<&str> = Vec::new();
    if inputs.is_away {
        states.push("Away");
    }
    // The reference's `AvatarDoNotDisturb` reads "Unavailable" — the state's
    // outward name, distinct from the menu entry that sets it.
    if inputs.is_do_not_disturb {
        states.push("Unavailable");
    }
    if inputs.is_autoresponse && toggles.show_autoresponse {
        states.push("Auto-Response");
    }
    // The reference shows this for the own and other avatars alike (the CUSTOMIZE
    // animation is signalled either way).
    if inputs.is_editing_appearance {
        states.push("(Editing Appearance)");
    }
    if inputs.is_muted && !inputs.is_self {
        states.push("Blocked");
    }
    if inputs.is_typing && toggles.show_typing && !inputs.is_self {
        states.push("Typing");
    }
    if !states.is_empty() {
        lines.push(TagLine {
            text: states.join(", "),
            size: TagLineSize::Small,
            color: base_color,
        });
    }

    // --- Group title (small, tag colour). ---
    if toggles.show_group_titles
        && let Some(title) = inputs.title
        && !title.is_empty()
    {
        lines.push(TagLine {
            text: title.to_owned(),
            size: TagLineSize::Small,
            color: base_color,
        });
    }

    // --- Name line (large, tag colour). ---
    let name = inputs
        .record
        .and_then(|record| {
            if toggles.show_display_names {
                record.preferred_name()
            } else {
                // Display names off still shows a user-given alias: the toggle
                // picks between the grid's two names, and an alias is neither.
                record.legacy_display_name()
            }
        })
        .map_or_else(|| inputs.provisional.clone(), str::to_owned);
    lines.push(TagLine {
        text: name,
        size: TagLineSize::Name,
        color: base_color,
    });

    // --- Username line (small, desaturated) under a custom display name. ---
    if toggles.show_usernames
        && has_custom_display_name
        && let Some(username) = inputs.record.and_then(|record| record.username.as_deref())
    {
        lines.push(TagLine {
            text: username.to_owned(),
            size: TagLineSize::Small,
            color: scale_rgb(base_color, USERNAME_DESATURATE),
        });
    }

    // --- Distance line (small, band colour; own-avatar distance, like the
    // reference — the fade/cut-off is what's camera-based). ---
    if toggles.show_distance
        && let Some(distance) = inputs.distance_m
    {
        lines.push(TagLine {
            text: format!("{distance:.2} m"),
            size: TagLineSize::Small,
            color: distance_band_color(distance, colors),
        });
    }

    // --- Render-cost lines (small): the ARC, rated against the budget, and —
    // only when the attachment area is what put the avatar over — the area that
    // did it. The reference shows the cost either way, noting that untangling
    // which limit fired would cost more than it explains.
    if shows_complexity(inputs.is_self, inputs.is_limited, toggles)
        && let Some(complexity) = inputs.complexity
    {
        lines.push(TagLine {
            text: format!("Complexity: {complexity}"),
            size: TagLineSize::Small,
            // Your own cost is reported, never rated: the limit does not apply
            // to you, so a red tag would be telling you off for nothing.
            color: if inputs.is_self {
                COMPLEXITY_UNRATED_COLOR
            } else {
                complexity_color(complexity, toggles.complexity_limit)
            },
        });
        if toggles.area_limit_m2 > 0.0 && inputs.attachment_area_m2 > toggles.area_limit_m2 {
            let area = inputs.attachment_area_m2.round();
            lines.push(TagLine {
                text: format!("Texture Area: {area:.0} m²"),
                size: TagLineSize::Small,
                color: TEXTURE_AREA_COLOR,
            });
        }
    }

    TagContent { lines, base_color }
}

/// The AWAY built-in animation's id, resolved once — the signalled-set entry
/// is the protocol's only carrier of another avatar's away state. Shared
/// with the avatar radar's away column.
pub(crate) static AWAY_ANIM: std::sync::LazyLock<Option<sl_client_bevy::Uuid>> =
    std::sync::LazyLock::new(|| {
        sl_anim::registry::builtin_animation_by_name("away").map(|animation| animation.id)
    });

/// The DO NOT DISTURB built-in animation's id, resolved once — like AWAY, the
/// signalled-set entry is the protocol's only carrier of another avatar's
/// unavailable state.
static DND_ANIM: std::sync::LazyLock<Option<sl_client_bevy::Uuid>> =
    std::sync::LazyLock::new(|| {
        sl_anim::registry::builtin_animation_by_name("do_not_disturb").map(|animation| animation.id)
    });

/// The CUSTOMIZE built-in animation's id, resolved once — its presence in an
/// avatar's signalled set is the protocol's carrier of "editing appearance".
static CUSTOMIZE_ANIM: std::sync::LazyLock<Option<sl_client_bevy::Uuid>> =
    std::sync::LazyLock::new(|| {
        sl_anim::registry::builtin_animation_by_name("customize").map(|animation| animation.id)
    });

/// How often the avatar-distance inputs refresh, seconds (avatars move most
/// frames — without a throttle every tag would recompose per frame).
const DISTANCE_REFRESH_SECS: f32 = 0.25;

/// The distance change, metres, below which a cached distance is kept (the
/// two-decimal display grain).
const DISTANCE_HYSTERESIS_METRES: f32 = 0.05;

/// Recompose every labelled avatar's [`TagContent`] from the live inputs
/// (names, title, friend/mute/typing/away state, own-avatar distance).
/// Assembly runs each frame but the final compare-then-assign is the
/// authoritative guard — the renderer only sees `Changed<TagContent>` when
/// something the tag *shows* actually changed; the distance additionally
/// refreshes at most at [`DISTANCE_REFRESH_SECS`] with a metre hysteresis.
#[expect(
    clippy::too_many_arguments,
    reason = "the composer is the single fan-in of every tag-content source; \
              splitting it would just move the arguments into a SystemParam \
              with the same width"
)]
pub(crate) fn compose_name_tags(
    time: Res<Time>,
    mut next_distance_at: Local<f32>,
    mut distance_cache: Local<std::collections::HashMap<AgentKey, f32>>,
    avatars: Res<crate::avatars::AvatarState>,
    statuses: Res<NameTagStatuses>,
    playback: Res<crate::animations::AnimationPlayback>,
    friends: Option<Res<crate::world_api::FriendsModel>>,
    mutes: Option<Res<crate::world_api::MuteModel>>,
    groups: Option<Res<crate::groups::GroupsModel>>,
    complexity: Option<Res<crate::avatar_complexity::AvatarComplexityModel>>,
    identity: Option<Res<sl_client_bevy::SlIdentity>>,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    anchors: Query<&Transform, With<crate::avatars::AvatarAnchor>>,
    mut contents: Query<&mut TagContent, With<crate::avatars::NameTag>>,
) {
    let toggles = TagToggles::from_settings(settings.as_deref());
    let colors = TagColors::from_settings(settings.as_deref());
    // Autorespond is local-only state, so it can only ever mark the own tag.
    let autoresponse = crate::presence::shows_autoresponse(settings.as_deref());
    let own_agent = identity.as_ref().and_then(|identity| identity.agent_id);
    // The distance line measures from the OWN AVATAR (the reference's
    // behaviour) — the camera-based distances only govern fade/cut-off.
    let own_position = own_agent
        .and_then(|own| {
            avatars
                .labelled_avatars()
                .find(|(agent, _, _)| *agent == own)
        })
        .and_then(|(_, anchor, _)| anchors.get(anchor).ok())
        .map(|transform| transform.translation);

    // Refresh the throttled avatar-distance cache when due.
    let now = time.elapsed_secs();
    if now >= *next_distance_at {
        *next_distance_at = now + DISTANCE_REFRESH_SECS;
        if let Some(own_position) = own_position {
            for (agent, anchor, _) in avatars.labelled_avatars() {
                let Ok(transform) = anchors.get(anchor) else {
                    continue;
                };
                let distance = own_position.distance(transform.translation);
                let cached = distance_cache.get(&agent).copied();
                if cached
                    .is_none_or(|cached| (cached - distance).abs() >= DISTANCE_HYSTERESIS_METRES)
                {
                    distance_cache.insert(agent, distance);
                }
            }
        }
    }

    for (agent, _, label) in avatars.labelled_avatars() {
        let Ok(mut content) = contents.get_mut(label) else {
            continue;
        };
        let is_self = own_agent == Some(agent);
        let title = if is_self {
            groups
                .as_ref()
                .and_then(|groups| groups.own_title())
                .or_else(|| avatars.title_of(agent))
        } else {
            avatars.title_of(agent)
        };
        let measured = complexity
            .as_ref()
            .and_then(|complexity| complexity.complexity(agent));
        let inputs = TagInputs {
            is_self,
            record: avatars.name_record(agent),
            provisional: avatars.label_text(agent),
            title,
            is_friend: friends
                .as_ref()
                .is_some_and(|friends| friends.is_friend(agent)),
            is_muted: mutes
                .as_ref()
                .is_some_and(|mutes| mutes.is_muted(agent.uuid())),
            is_typing: statuses.is_typing(agent),
            is_away: AWAY_ANIM.is_some_and(|away| playback.is_playing(agent, away)),
            is_do_not_disturb: DND_ANIM.is_some_and(|busy| playback.is_playing(agent, busy)),
            is_autoresponse: is_self && autoresponse,
            is_editing_appearance: CUSTOMIZE_ANIM
                .is_some_and(|customize| playback.is_playing(agent, customize)),
            distance_m: if is_self {
                None
            } else {
                distance_cache.get(&agent).copied()
            },
            complexity: measured.map(|cost| cost.score),
            attachment_area_m2: measured.map_or(0.0, |cost| cost.surface_area),
            is_limited: complexity
                .as_ref()
                .is_some_and(|complexity| complexity.is_jellied(agent)),
        };
        let composed = compose_tag(&inputs, &toggles, &colors);
        if *content != composed {
            *content = composed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMPLEXITY_UNRATED_COLOR, DISTANCE_BEYOND_COLOR, DISTANCE_CHAT_COLOR, DISTANCE_SHOUT_COLOR,
        DISTANCE_WHISPER_COLOR, NAME_TAG_FRIEND, NAME_TAG_LINDEN, NAME_TAG_MISMATCH,
        NAME_TAG_MUTED, TEXTURE_AREA_COLOR, TagColors, TagInputs, TagLineSize, TagToggles,
        complexity_color, compose_tag, distance_band_color, shows_complexity,
    };
    use crate::avatars::NameRecord;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A record with a custom display name (username line shows).
    fn custom_record() -> NameRecord {
        NameRecord {
            legacy: Some("Avatar Tester".to_owned()),
            username: Some("avatar.tester".to_owned()),
            display_name: Some("Shiny Name".to_owned()),
            is_display_name_default: false,
            alias: None,
        }
    }

    /// The composed line texts, in order.
    fn texts(content: &super::TagContent) -> Vec<&str> {
        content
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect()
    }

    /// Everything on: status, title, name, username, distance — in the
    /// reference's order, with the reference's size tiers.
    #[test]
    fn full_tag_line_order() {
        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            title: Some("Crew Chief"),
            is_typing: true,
            is_away: true,
            distance_m: Some(12.34),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default(), &TagColors::default());
        assert_eq!(
            texts(&content),
            vec![
                "Away, Typing",
                "Crew Chief",
                "Shiny Name",
                "avatar.tester",
                "12.34 m",
            ],
        );
        let sizes: Vec<TagLineSize> = content.lines.iter().map(|line| line.size).collect();
        assert_eq!(
            sizes,
            vec![
                TagLineSize::Small,
                TagLineSize::Small,
                TagLineSize::Name,
                TagLineSize::Small,
                TagLineSize::Small,
            ],
        );
        // A custom display name renders on the LtGray mismatch base.
        assert_eq!(content.base_color, NAME_TAG_MISMATCH);
    }

    /// Toggles with the render-cost line shown for everyone, rated against a
    /// budget of 100k.
    fn complexity_toggles() -> TagToggles {
        TagToggles {
            complexity_when_limited_only: false,
            complexity_limit: 100_000,
            ..TagToggles::default()
        }
    }

    /// The reference's three-setting rule: the master switch gates everything,
    /// your own tag needs its own opt-in, and other avatars show the line
    /// either always or only while the viewer is limiting them.
    #[test]
    fn complexity_line_visibility_rule() {
        let default = TagToggles::default();
        // Out of the box: nothing, until an avatar is actually being limited.
        assert!(!shows_complexity(false, false, &default));
        assert!(shows_complexity(false, true, &default));
        assert!(!shows_complexity(true, false, &default), "own tag opts in");

        let own = TagToggles {
            show_own_complexity: true,
            ..default
        };
        assert!(shows_complexity(true, false, &own));

        let everyone = TagToggles {
            complexity_when_limited_only: false,
            ..default
        };
        assert!(shows_complexity(false, false, &everyone));

        let off = TagToggles {
            show_complexity: false,
            show_own_complexity: true,
            complexity_when_limited_only: false,
            ..default
        };
        assert!(
            !shows_complexity(false, true, &off),
            "the master switch wins"
        );
        assert!(!shows_complexity(true, false, &off));
    }

    /// The cost ramps green → amber → red against the budget, and is reported
    /// in neutral grey when there is no budget to rate it against.
    #[test]
    fn complexity_color_ramps_against_the_budget() {
        assert_eq!(
            complexity_color(500_000, 0),
            COMPLEXITY_UNRATED_COLOR,
            "no budget means nothing to judge"
        );
        let srgb = |color: Color| {
            let linear = Srgba::from(color);
            (linear.red, linear.green, linear.blue)
        };
        assert_eq!(srgb(complexity_color(0, 100_000)), (0.0, 1.0, 0.0));
        assert_eq!(srgb(complexity_color(100_000, 100_000)), (1.0, 1.0, 0.0));
        assert_eq!(srgb(complexity_color(200_000, 100_000)), (1.0, 0.0, 0.0));
        assert_eq!(
            srgb(complexity_color(400_000, 100_000)),
            (1.0, 0.0, 0.0),
            "the ramp saturates rather than running past red"
        );
    }

    /// The cost line follows the distance line, carries the ramp colour, and is
    /// suppressed entirely for an avatar that has not been scored yet — a
    /// missing measurement must not read as a cost of zero.
    #[test]
    fn complexity_line_follows_the_distance_line() {
        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            distance_m: Some(5.0),
            complexity: Some(250_000),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &complexity_toggles(), &TagColors::default());
        assert_eq!(
            texts(&content),
            vec![
                "Shiny Name",
                "avatar.tester",
                "5.00 m",
                "Complexity: 250000"
            ],
        );
        assert_eq!(
            content.lines.last().map(|line| line.color),
            Some(complexity_color(250_000, 100_000)),
        );

        let unscored = TagInputs {
            record: Some(&record),
            complexity: None,
            ..TagInputs::default()
        };
        let content = compose_tag(&unscored, &complexity_toggles(), &TagColors::default());
        assert!(
            !texts(&content)
                .iter()
                .any(|line| line.starts_with("Complexity")),
            "an unscored avatar shows no cost line at all"
        );
    }

    /// The texture-area line appears only when the attachment area is over the
    /// limit, always in red — and never when the area limit is off.
    #[test]
    fn texture_area_line_is_the_complaint_it_looks_like() {
        let toggles = TagToggles {
            area_limit_m2: 1000.0,
            ..complexity_toggles()
        };
        let under = TagInputs {
            complexity: Some(10),
            attachment_area_m2: 999.0,
            ..TagInputs::default()
        };
        assert!(
            !texts(&compose_tag(&under, &toggles, &TagColors::default()))
                .iter()
                .any(|line| line.starts_with("Texture Area")),
        );

        let over = TagInputs {
            attachment_area_m2: 2500.4,
            ..under
        };
        let content = compose_tag(&over, &toggles, &TagColors::default());
        let area = content
            .lines
            .iter()
            .find(|line| line.text.starts_with("Texture Area"));
        assert_eq!(
            area.map(|line| (line.text.as_str(), line.color)),
            Some(("Texture Area: 2500 m²", TEXTURE_AREA_COLOR)),
            "the area line shows once the limit is passed"
        );

        // With the area limit off, no amount of area produces the line.
        let unlimited = TagToggles {
            area_limit_m2: 0.0,
            ..toggles
        };
        assert!(
            !texts(&compose_tag(&over, &unlimited, &TagColors::default()))
                .iter()
                .any(|line| line.starts_with("Texture Area")),
        );
    }

    /// Your own cost is reported in neutral grey, never rated red — the limit
    /// does not apply to you, so a red tag would be telling you off for nothing.
    #[test]
    fn your_own_cost_is_reported_not_rated() {
        let toggles = TagToggles {
            show_own_complexity: true,
            ..complexity_toggles()
        };
        let inputs = TagInputs {
            is_self: true,
            complexity: Some(500_000),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &toggles, &TagColors::default());
        let line = content
            .lines
            .iter()
            .find(|line| line.text.starts_with("Complexity"));
        assert_eq!(
            line.map(|line| (line.text.as_str(), line.color)),
            Some(("Complexity: 500000", COMPLEXITY_UNRATED_COLOR)),
            "the own tag reports its cost, unrated, once opted in"
        );
    }

    /// A default display name shows without a username line; display names display names
    /// off falls back to the legacy name.
    #[test]
    fn display_name_fallbacks() {
        let mut record = custom_record();
        record.is_display_name_default = true;
        record.display_name = Some("Avatar Tester".to_owned());
        let inputs = TagInputs {
            record: Some(&record),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default(), &TagColors::default());
        assert_eq!(texts(&content), vec!["Avatar Tester"]);

        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            ..TagInputs::default()
        };
        let toggles = TagToggles {
            show_display_names: false,
            ..TagToggles::default()
        };
        let content = compose_tag(&inputs, &toggles, &TagColors::default());
        assert_eq!(texts(&content), vec!["Avatar Tester"]);
    }

    /// No record at all → the provisional id fragment.
    #[test]
    fn provisional_name_without_record() {
        let inputs = TagInputs {
            provisional: "cafebabe".to_owned(),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default(), &TagColors::default());
        assert_eq!(texts(&content), vec!["cafebabe"]);
    }

    /// Colour precedence: friend > muted > Linden > display base; the friend
    /// gate falls through when off; self never shows Blocked/Typing/distance.
    #[test]
    fn colour_precedence_and_self_suppression() {
        let record = custom_record();
        let base = TagInputs {
            record: Some(&record),
            is_friend: true,
            is_muted: true,
            ..TagInputs::default()
        };
        assert_eq!(
            compose_tag(&base, &TagToggles::default(), &TagColors::default()).base_color,
            NAME_TAG_FRIEND
        );
        let toggles_no_friend = TagToggles {
            show_friend_color: false,
            ..TagToggles::default()
        };
        assert_eq!(
            compose_tag(&base, &toggles_no_friend, &TagColors::default()).base_color,
            NAME_TAG_MUTED
        );

        let linden_record = NameRecord {
            legacy: Some("Kindly Linden".to_owned()),
            ..NameRecord::default()
        };
        let linden = TagInputs {
            record: Some(&linden_record),
            ..TagInputs::default()
        };
        assert_eq!(
            compose_tag(&linden, &TagToggles::default(), &TagColors::default()).base_color,
            NAME_TAG_LINDEN
        );

        // Self: muted/typing/distance are all suppressed.
        let own = TagInputs {
            record: Some(&record),
            is_self: true,
            is_muted: true,
            is_typing: true,
            distance_m: None,
            ..TagInputs::default()
        };
        let content = compose_tag(&own, &TagToggles::default(), &TagColors::default());
        assert_eq!(texts(&content), vec!["Shiny Name", "avatar.tester"],);
    }

    /// The `(Editing Appearance)` status shows for the own avatar too (only
    /// Blocked / Typing are self-suppressed), joined after Away.
    #[test]
    fn editing_appearance_status_shows() {
        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            is_self: true,
            is_away: true,
            is_editing_appearance: true,
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default(), &TagColors::default());
        assert_eq!(
            content.lines.first().map(|line| line.text.as_str()),
            Some("Away, (Editing Appearance)"),
        );
    }

    /// The presence statuses read as the reference's outward names and join in
    /// its order: `Unavailable` for do-not-disturb, `Auto-Response` for an
    /// autorespond mode, both after `Away`.
    #[test]
    fn presence_statuses_use_the_reference_wording() {
        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            is_self: true,
            is_away: true,
            is_do_not_disturb: true,
            is_autoresponse: true,
            ..TagInputs::default()
        };
        let toggles = TagToggles {
            show_autoresponse: true,
            ..TagToggles::default()
        };
        let content = compose_tag(&inputs, &toggles, &TagColors::default());
        assert_eq!(
            content.lines.first().map(|line| line.text.as_str()),
            Some("Away, Unavailable, Auto-Response"),
        );
        // The autorespond entry is off by default, like the reference's
        // `FSShowAutorespondInNametag`; the wire-carried states are not.
        let quiet = compose_tag(&inputs, &TagToggles::default(), &TagColors::default());
        assert_eq!(
            quiet.lines.first().map(|line| line.text.as_str()),
            Some("Away, Unavailable"),
        );
    }

    /// The distance bands at their boundaries, and the two-decimal format.
    #[test]
    fn distance_bands_and_format() {
        let colors = TagColors::default();
        assert_eq!(distance_band_color(9.99, &colors), DISTANCE_WHISPER_COLOR);
        assert_eq!(distance_band_color(10.01, &colors), DISTANCE_CHAT_COLOR);
        assert_eq!(distance_band_color(20.01, &colors), DISTANCE_SHOUT_COLOR);
        assert_eq!(distance_band_color(100.01, &colors), DISTANCE_BEYOND_COLOR);

        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            distance_m: Some(150.0),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default(), &TagColors::default());
        let distance_line = content.lines.last().cloned();
        assert_eq!(
            distance_line.as_ref().map(|line| line.text.as_str()),
            Some("150.00 m")
        );
        assert_eq!(
            distance_line.map(|line| line.color),
            Some(DISTANCE_BEYOND_COLOR)
        );
    }

    /// An overridden palette colour lands on the tag: a friend's tag takes
    /// the palette's friend slot, not the built-in constant.
    #[test]
    fn overridden_palette_reaches_the_tag() {
        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            is_friend: true,
            ..TagInputs::default()
        };
        let colors = TagColors {
            friend: Color::srgb(0.1, 0.2, 0.9),
            ..TagColors::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default(), &colors);
        assert_eq!(content.base_color, Color::srgb(0.1, 0.2, 0.9));
    }

    /// The change-detection contract: identical inputs compose an equal
    /// value (so the compare-then-assign writers stay quiet).
    #[test]
    fn equal_inputs_compose_equal_content() {
        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            title: Some("Crew Chief"),
            distance_m: Some(5.0),
            ..TagInputs::default()
        };
        assert_eq!(
            compose_tag(&inputs, &TagToggles::default(), &TagColors::default()),
            compose_tag(&inputs, &TagToggles::default(), &TagColors::default()),
        );
    }
}
