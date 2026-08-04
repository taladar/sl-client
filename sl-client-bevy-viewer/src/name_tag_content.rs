//! Composed avatar name-tag **content** — the lines a tag shows and their
//! colours (the reference viewer's `LLVOAvatar::idleUpdateNameTagText`).
//!
//! This module owns the *what* of a tag; the *how* (world-space billboard
//! rendering) lives in [`crate::name_tag_billboard`]. The renderer consumes a
//! [`TagContent`] component on each tag entity and rebuilds text layout and
//! mesh only when the composed value actually changes, so composition here is
//! deliberately change-driven: assemble, compare, and only then assign.

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

/// Tint the whole tag by chat-range band (Firestorm
/// `FSTagShowDistanceColors`, default off).
pub(crate) const SETTING_COLOR_BY_DISTANCE: &str = "ColorByDistance";

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

/// Base tag colour with display names off (`NameTagLegacy`, White).
const NAME_TAG_LEGACY: Color = Color::WHITE;

/// Base tag colour for a *default* display name (`NameTagMatch`, White).
const NAME_TAG_MATCH: Color = Color::WHITE;

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
    /// Own-avatar→avatar distance, metres; `None` suppresses the distance
    /// line (the own tag, or the own avatar not being placed yet).
    pub(crate) distance_m: Option<f32>,
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
    /// [`SETTING_COLOR_BY_DISTANCE`].
    pub(crate) color_by_distance: bool,
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
            color_by_distance: false,
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
            color_by_distance: get(SETTING_COLOR_BY_DISTANCE, false),
        }
    }
}

/// The chat-range band colour for a distance (the reference's
/// whisper / say / shout / beyond bands).
fn distance_band_color(distance_m: f32) -> Color {
    if distance_m <= WHISPER_RANGE_METRES {
        DISTANCE_WHISPER_COLOR
    } else if distance_m <= CHAT_RANGE_METRES {
        DISTANCE_CHAT_COLOR
    } else if distance_m <= SHOUT_RANGE_METRES {
        DISTANCE_SHOUT_COLOR
    } else {
        DISTANCE_BEYOND_COLOR
    }
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
/// `Away` / `Blocked` / `Typing`), group title, name, username, distance.
pub(crate) fn compose_tag(inputs: &TagInputs<'_>, toggles: &TagToggles) -> TagContent {
    // --- The whole-tag colour (the reference's precedence chain). ---
    let has_custom_display_name = toggles.show_display_names
        && inputs
            .record
            .is_some_and(|record| record.display_name.is_some() && !record.is_display_name_default);
    let display_base = if !toggles.show_display_names
        || inputs
            .record
            .is_none_or(|record| record.display_name.is_none())
    {
        NAME_TAG_LEGACY
    } else if has_custom_display_name {
        NAME_TAG_MISMATCH
    } else {
        NAME_TAG_MATCH
    };
    let base_color = if inputs.is_self {
        NAME_TAG_SELF
    } else if inputs.is_friend && toggles.show_friend_color {
        NAME_TAG_FRIEND
    } else if inputs.is_muted {
        NAME_TAG_MUTED
    } else if is_linden(inputs.record) {
        NAME_TAG_LINDEN
    } else if toggles.color_by_distance {
        // The whole-tag distance tint applies only when no identity colour
        // (self / friend) claimed the tag.
        inputs.distance_m.map_or(display_base, distance_band_color)
    } else {
        display_base
    };

    let mut lines = Vec::new();

    // --- Status line (small, tag colour): Away, Blocked, Typing. ---
    let mut states: Vec<&str> = Vec::new();
    if inputs.is_away {
        states.push("Away");
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
                record.legacy.as_deref()
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
            color: distance_band_color(distance),
        });
    }

    TagContent { lines, base_color }
}

/// The AWAY built-in animation's id, resolved once — the signalled-set entry
/// is the protocol's only carrier of another avatar's away state.
static AWAY_ANIM: std::sync::LazyLock<Option<sl_client_bevy::Uuid>> =
    std::sync::LazyLock::new(|| {
        sl_anim::registry::builtin_animation_by_name("away").map(|animation| animation.id)
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
    friends: Option<Res<crate::people::FriendsModel>>,
    mutes: Option<Res<crate::mutes::MuteModel>>,
    groups: Option<Res<crate::groups::GroupsModel>>,
    identity: Option<Res<sl_client_bevy::SlIdentity>>,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    anchors: Query<&Transform, With<crate::avatars::AvatarAnchor>>,
    mut contents: Query<&mut TagContent, With<crate::avatars::NameTag>>,
) {
    let toggles = TagToggles::from_settings(settings.as_deref());
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
            distance_m: if is_self {
                None
            } else {
                distance_cache.get(&agent).copied()
            },
        };
        let composed = compose_tag(&inputs, &toggles);
        if *content != composed {
            *content = composed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DISTANCE_BEYOND_COLOR, DISTANCE_CHAT_COLOR, DISTANCE_SHOUT_COLOR, DISTANCE_WHISPER_COLOR,
        NAME_TAG_FRIEND, NAME_TAG_LINDEN, NAME_TAG_MISMATCH, NAME_TAG_MUTED, TagInputs,
        TagLineSize, TagToggles, compose_tag, distance_band_color,
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
        let content = compose_tag(&inputs, &TagToggles::default());
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

    /// A default display name shows without a username line; display names
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
        let content = compose_tag(&inputs, &TagToggles::default());
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
        let content = compose_tag(&inputs, &toggles);
        assert_eq!(texts(&content), vec!["Avatar Tester"]);
    }

    /// No record at all → the provisional id fragment.
    #[test]
    fn provisional_name_without_record() {
        let inputs = TagInputs {
            provisional: "cafebabe".to_owned(),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default());
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
            compose_tag(&base, &TagToggles::default()).base_color,
            NAME_TAG_FRIEND
        );
        let toggles_no_friend = TagToggles {
            show_friend_color: false,
            ..TagToggles::default()
        };
        assert_eq!(
            compose_tag(&base, &toggles_no_friend).base_color,
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
            compose_tag(&linden, &TagToggles::default()).base_color,
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
        let content = compose_tag(&own, &TagToggles::default());
        assert_eq!(texts(&content), vec!["Shiny Name", "avatar.tester"],);
    }

    /// The distance bands at their boundaries, and the two-decimal format.
    #[test]
    fn distance_bands_and_format() {
        assert_eq!(distance_band_color(9.99), DISTANCE_WHISPER_COLOR);
        assert_eq!(distance_band_color(10.01), DISTANCE_CHAT_COLOR);
        assert_eq!(distance_band_color(20.01), DISTANCE_SHOUT_COLOR);
        assert_eq!(distance_band_color(100.01), DISTANCE_BEYOND_COLOR);

        let record = custom_record();
        let inputs = TagInputs {
            record: Some(&record),
            distance_m: Some(150.0),
            ..TagInputs::default()
        };
        let content = compose_tag(&inputs, &TagToggles::default());
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
            compose_tag(&inputs, &TagToggles::default()),
            compose_tag(&inputs, &TagToggles::default()),
        );
    }
}
