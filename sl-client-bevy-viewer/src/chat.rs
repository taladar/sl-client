//! On-screen local-chat overlay: a `bevy_ui` column of transient text lines
//! pinned to the bottom-left corner that shows recent nearby chat over the world.
//!
//! This is the Phase 11 slice — a read-only overlay, no input box — extended by
//! [`viewer-chat-overlay-fade`] to decay like the reference viewer's floating
//! nearby-chat toasts. Each
//! [`ChatReceived`](sl_client_bevy::SlSessionEvent::ChatReceived) message
//! (`ChatFromSimulator`: a nearby agent or object speaking) is formatted as
//! `"{from_name}: {message}"` and spawned as its own [`ChatOverlayLine`] text
//! node, appended to the bottom of the column so the newest line sits lowest. A
//! whisper or a shout carries a short prefix label so the volume is
//! distinguishable; a normal say has none.
//!
//! Unlike the Phase 11 single joined-string node, each line is its own entity
//! carrying its own [`age`](ChatOverlayLine::age): a line appears fully opaque,
//! holds for [`CHAT_HOLD_TIME`], then fades over [`CHAT_FADE_DURATION`] and is
//! despawned once fully transparent, so the corner empties itself again once chat
//! goes quiet. A newly arriving line never disturbs the ages of lines already
//! fading — each line's own age drives its alpha independently. The persistent,
//! interactive scrollback lives in the Conversations Nearby tab
//! ([`viewer-chat-history-panel`]); this overlay is the transient heads-up display.
//!
//! The age advances by frame-time ([`Time::delta_secs`]), never wall-clock, so it
//! is deterministic under the screenshot harness's manual clock. The overlay needs
//! no name resolution — the simulator already supplies the speaker's display name
//! in [`ChatMessage::from_name`](sl_client_bevy::ChatMessage).

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, ChatMessage, ChatSource, ChatType, SlEvent, SlIdentity, SlSessionEvent,
};

use crate::preferences_chat::{
    SETTING_CHAT_FONT_SIZE, SETTING_CHAT_MAX_LINES, SETTING_NEARBY_TOAST_LIFETIME,
};
use crate::settings::ViewerSettings;
use crate::ui::BottomArea;
use crate::ui::UiRoot;
use crate::ui_font::UiFont;
use crate::world_api::LocalChatNotice;

/// The most chat lines the overlay ever shows at once when no
/// [`SETTING_CHAT_MAX_LINES`] value is available. Fading already bounds each
/// line's lifetime; this is the burst safety valve, evicting the oldest line so a
/// flood of near-simultaneous chat cannot grow the column without bound.
const CHAT_MAX_LINES: usize = 12;

/// How long, in seconds, a freshly arrived line stays fully opaque before it
/// begins to fade, when no [`SETTING_NEARBY_TOAST_LIFETIME`] value is
/// available. Reference-faithful: Firestorm's `NearbyToastLifeTime` (23 s)
/// minus its `NearbyToastFadingTime` (3 s).
const CHAT_HOLD_TIME: f32 = 20.0;

/// How long, in seconds, a line takes to fade from fully opaque to fully
/// transparent once its hold time lapses. Matches Firestorm's
/// `NearbyToastFadingTime`.
const CHAT_FADE_DURATION: f32 = 3.0;

/// The overlay font size, in logical pixels, at the medium
/// [`SETTING_CHAT_FONT_SIZE`] step (and when no settings are available).
const CHAT_FONT_SIZE: f32 = 15.0;

/// The overlay font size for the stored [`SETTING_CHAT_FONT_SIZE`] step:
/// `0` small, `1` medium, `2` large (an unknown step reads as medium).
const fn overlay_font_size(step: u32) -> f32 {
    match step {
        0 => 13.0,
        2 => 17.0,
        _medium => CHAT_FONT_SIZE,
    }
}

/// The stored [`SETTING_CHAT_FONT_SIZE`] step (medium when no settings are
/// available, e.g. a bare headless test world).
fn font_step(settings: Option<&ViewerSettings>) -> u32 {
    settings
        .and_then(|settings| settings.store().get_u32(SETTING_CHAT_FONT_SIZE).ok())
        .unwrap_or(1)
}

/// The stored hold time: [`SETTING_NEARBY_TOAST_LIFETIME`] (the full on-screen
/// lifetime) minus the fade, floored at zero; [`CHAT_HOLD_TIME`] when no
/// settings are available.
fn hold_time(settings: Option<&ViewerSettings>) -> f32 {
    settings
        .and_then(|settings| settings.store().get_u32(SETTING_NEARBY_TOAST_LIFETIME).ok())
        .and_then(|seconds| u16::try_from(seconds).ok())
        .map_or(CHAT_HOLD_TIME, |seconds| {
            (f32::from(seconds) - CHAT_FADE_DURATION).max(0.0)
        })
}

/// The stored overlay line cap ([`SETTING_CHAT_MAX_LINES`], floored at one);
/// [`CHAT_MAX_LINES`] when no settings are available.
fn max_lines(settings: Option<&ViewerSettings>) -> usize {
    settings
        .and_then(|settings| settings.store().get_u32(SETTING_CHAT_MAX_LINES).ok())
        .and_then(|lines| usize::try_from(lines).ok())
        .map_or(CHAT_MAX_LINES, |lines| lines.max(1))
}

/// The inset, in logical pixels, of the overlay from the left edge.
const CHAT_INSET: f32 = 10.0;

/// The overlay's initial distance from the bottom edge, in logical pixels, used
/// until the bottom area has been measured — [`position_chat_overlay`] then keeps
/// it just above the whole bottom area (toolbar + nearby-chat bar) so the two never
/// overlap, whatever the bar's height or whether it is toggled off.
const CHAT_BOTTOM_INSET: f32 = 48.0;

/// The gap kept between the top of the bottom area and the overlay's lowest line,
/// in logical pixels.
const CHAT_OVERLAY_GAP: f32 = 6.0;

/// A marker component tagging the overlay's column container, so the positioning
/// system can find and re-anchor it and new lines can be parented to it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ChatOverlayContainer;

/// Which palette colour an overlay line renders in, classified once at
/// arrival — the user-tunable chat colours of the preferences colors & skins
/// tab ([`crate::skin_colors`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatLineSource {
    /// The own avatar spoke ([`crate::skin_colors::SETTING_CHAT_SELF`]).
    Own,
    /// Another avatar spoke ([`crate::skin_colors::SETTING_CHAT_OTHERS`]).
    Others,
    /// An in-world object spoke ([`crate::skin_colors::SETTING_CHAT_OBJECTS`]).
    Objects,
    /// The system / region, an unknown source, or a viewer-generated notice
    /// ([`crate::skin_colors::SETTING_CHAT_SYSTEM`]).
    System,
}

/// One transient chat line in the overlay: a text node under the
/// [`ChatOverlayContainer`] that ages, fades, and despawns on its own.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ChatOverlayLine {
    /// Frame-time seconds elapsed since this line arrived. Drives the alpha and,
    /// once it passes hold + fade, the despawn. Only ever advanced by
    /// [`Time::delta_secs`], so it is deterministic under the manual clock.
    age: f32,
    /// Monotonic arrival order (oldest = smallest), so an overflow beyond
    /// [`CHAT_MAX_LINES`] evicts the oldest line deterministically even when
    /// several lines share the same age.
    seq: u64,
    /// The line's colour classification, fixed at arrival; the per-frame tick
    /// re-resolves the palette so a live picker drag recolours floating lines.
    source: ChatLineSource,
}

/// The overlay's only mutable state: the next arrival sequence number to stamp on
/// a line. The lines themselves are entities, not a buffer here.
#[derive(Resource, Default)]
pub(crate) struct ChatOverlay {
    /// The sequence number the next arriving line will be stamped with.
    next_seq: u64,
}

/// Format one chat message as an overlay line: `"{from_name}: {message}"`, with
/// a short volume label prefixed for a whisper or a shout (a normal say has
/// none).
fn format_chat_line(message: &ChatMessage) -> String {
    let body = format!("{}: {}", message.from_name, message.message);
    match message.chat_type {
        ChatType::Whisper => format!("[whisper] {body}"),
        ChatType::Shout => format!("[shout] {body}"),
        _other => body,
    }
}

/// Classify a received message's speaker for the line colour: the own avatar,
/// another avatar, an object, or the system (unknown sources read as system,
/// the defensive default).
fn classify_source(message: &ChatMessage, own_agent: Option<AgentKey>) -> ChatLineSource {
    match &message.source {
        ChatSource::Agent(agent) if Some(*agent) == own_agent => ChatLineSource::Own,
        ChatSource::Agent(_) => ChatLineSource::Others,
        ChatSource::Object(_) => ChatLineSource::Objects,
        ChatSource::System | ChatSource::Unknown { .. } => ChatLineSource::System,
    }
}

/// A line's palette colour (opaque; the fade owns the alpha), re-resolved from
/// the store so an edit on the colors & skins tab applies live.
fn line_color(source: ChatLineSource, settings: Option<&ViewerSettings>) -> Color {
    let name = match source {
        ChatLineSource::Own => crate::skin_colors::SETTING_CHAT_SELF,
        ChatLineSource::Others => crate::skin_colors::SETTING_CHAT_OTHERS,
        ChatLineSource::Objects => crate::skin_colors::SETTING_CHAT_OBJECTS,
        ChatLineSource::System => crate::skin_colors::SETTING_CHAT_SYSTEM,
    };
    crate::skin_colors::setting_color(settings, name)
}

/// A line's alpha from its age: fully opaque through `hold` seconds, then a
/// linear ramp down to `0.0` over [`CHAT_FADE_DURATION`], clamped to `[0, 1]`.
fn line_alpha(age: f32, hold: f32) -> f32 {
    if age <= hold {
        1.0
    } else {
        let faded = (age - hold) / CHAT_FADE_DURATION;
        (1.0 - faded).clamp(0.0, 1.0)
    }
}

/// Whether a line has fully faded and should be despawned.
fn is_faded(age: f32, hold: f32) -> bool {
    age >= hold + CHAT_FADE_DURATION
}

/// Whether a received chat message should appear in the overlay: only messages
/// that carry text. The typing-animation triggers arrive as
/// [`ChatTyping`](sl_client_bevy::SlSessionEvent::ChatTyping) rather than
/// `ChatReceived`, but an empty-text message (or a stray typing type) is skipped
/// defensively so blank lines never accumulate.
const fn is_displayable(message: &ChatMessage) -> bool {
    !matches!(
        message.chat_type,
        ChatType::StartTyping | ChatType::StopTyping
    ) && !message.message.is_empty()
}

/// Startup system: spawn the overlay's column container, pinned to the
/// bottom-left corner. It starts empty; each arriving line is spawned as a child
/// and stacks upward, so the newest line stays at the bottom.
///
/// Parented under [`UiRoot`] so the snapshot floater's include-UI-off hide
/// (`Display::None` on `UiRoot`) covers it like every other panel — otherwise the
/// overlay's transient lines (including the "snapshot saved" notice) leak into a
/// clean-world-view shot. It stays absolutely positioned, so anchoring against the
/// full-window root is identical to anchoring against the window
/// ([`position_chat_overlay`] finds it by marker, not by parent, so it is
/// unaffected).
pub(crate) fn setup_chat_overlay(mut commands: Commands, root: Res<UiRoot>) {
    commands.spawn((
        // Anchored at the bottom-left with auto size, so the column grows upward as
        // lines are added; children stack top-to-bottom, newest appended last (and
        // thus lowest).
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(CHAT_INSET),
            bottom: Val::Px(CHAT_BOTTOM_INSET),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        // A read-only heads-up overlay must never eat clicks: without this it
        // blocks by default (`should_block_lower` defaults to `true` on a node with
        // no `Pickable`), so its transient lines silently suppress world picking
        // (touch, and the avatar context menu's body pick) wherever they float.
        Pickable::IGNORE,
        ChatOverlayContainer,
        ChildOf(root.0),
        Name::new("chat-overlay"),
    ));
}

/// Keep the overlay pinned just above the whole bottom area (the toolbar plus the
/// nearby-chat bar), reading the area's measured height so the clearance follows
/// the bar growing, shrinking, or being toggled off — no fixed magic number that
/// only fits one bar layout.
///
/// Reads last frame's [`ComputedNode`] (laid out in `PostUpdate`); the bottom area
/// changes height rarely, so a frame-old measurement never lets the two overlap by
/// more than a hair. Inert until the bottom area exists.
pub(crate) fn position_chat_overlay(
    bottom_area: Option<Res<BottomArea>>,
    computed: Query<&ComputedNode>,
    mut overlays: Query<&mut Node, With<ChatOverlayContainer>>,
) {
    let Some(bottom_area) = bottom_area else {
        return;
    };
    let Ok(node) = computed.get(bottom_area.area) else {
        return;
    };
    let height = node.size().y * node.inverse_scale_factor();
    if height <= 0.0 {
        return;
    }
    let wanted = Val::Px(height + CHAT_OVERLAY_GAP);
    if let Ok(mut overlay) = overlays.single_mut()
        && overlay.bottom != wanted
    {
        overlay.bottom = wanted;
    }
}

/// Spawn a fresh, fully-opaque [`ChatOverlayLine`] under the container for each
/// displayable local-chat message that arrives this frame.
pub(crate) fn update_chat_overlay(
    mut commands: Commands,
    mut events: MessageReader<SlEvent>,
    mut notices: MessageReader<LocalChatNotice>,
    mut overlay: ResMut<ChatOverlay>,
    container: Query<Entity, With<ChatOverlayContainer>>,
    settings: Option<Res<ViewerSettings>>,
    identity: Option<Res<SlIdentity>>,
) {
    let Ok(container) = container.single() else {
        return;
    };
    let font_size = overlay_font_size(font_step(settings.as_deref()));
    let own_agent = identity.as_ref().and_then(|identity| identity.agent_id);
    let spawn_line = |line: String,
                      source: ChatLineSource,
                      overlay: &mut ChatOverlay,
                      commands: &mut Commands| {
        debug!("chat overlay: {line}");
        let seq = overlay.next_seq;
        overlay.next_seq = overlay.next_seq.wrapping_add(1);
        commands.spawn((
            Text::new(line),
            UiFont::Sans.at(font_size),
            TextColor(line_color(source, settings.as_deref())),
            // Transparent to picks, like its container: a fading chat line must
            // not block a world click that happens to land on it.
            Pickable::IGNORE,
            ChatOverlayLine {
                age: 0.0,
                seq,
                source,
            },
            ChildOf(container),
        ));
    };
    for event in events.read() {
        if let SlSessionEvent::ChatReceived(message) = &event.0
            && is_displayable(message)
        {
            spawn_line(
                format_chat_line(message),
                classify_source(message, own_agent),
                &mut overlay,
                &mut commands,
            );
        }
    }
    // Client-generated notices (build-tool alerts, etc.) render as overlay lines
    // too, so viewer feedback shares the on-screen local-chat surface.
    for notice in notices.read() {
        spawn_line(
            notice.text.clone(),
            ChatLineSource::System,
            &mut overlay,
            &mut commands,
        );
    }
}

/// Advance every line's age by this frame's delta, drive each line's colour
/// (its palette colour at the age's alpha) from its own age, despawn lines
/// that have fully faded, and evict the oldest lines beyond the stored line
/// cap so a burst cannot grow the column without bound. The hold time, cap
/// and palette re-resolve from the settings every frame, so a preference
/// change — including a live colour-picker drag — governs the
/// already-floating lines too.
pub(crate) fn tick_chat_overlay(
    mut commands: Commands,
    time: Res<Time>,
    mut lines: Query<(Entity, &mut ChatOverlayLine, &mut TextColor)>,
    settings: Option<Res<ViewerSettings>>,
) {
    let dt = time.delta_secs();
    let hold = hold_time(settings.as_deref());
    let cap = max_lines(settings.as_deref());
    // Surviving (not-yet-faded) lines with their arrival order, for the overflow
    // pass below.
    let mut survivors: Vec<(Entity, u64)> = Vec::new();
    for (entity, mut line, mut color) in &mut lines {
        line.age += dt;
        if is_faded(line.age, hold) {
            commands.entity(entity).despawn();
            continue;
        }
        let wanted =
            line_color(line.source, settings.as_deref()).with_alpha(line_alpha(line.age, hold));
        if color.0 != wanted {
            color.0 = wanted;
        }
        survivors.push((entity, line.seq));
    }
    if survivors.len() > cap {
        survivors.sort_unstable_by_key(|&(_, seq)| seq);
        let overflow = survivors.len().saturating_sub(cap);
        for &(entity, _) in survivors.iter().take(overflow) {
            commands.entity(entity).despawn();
        }
    }
}

/// Restyle the already-floating overlay lines when the stored font-size step
/// changes (the combo applies live through the store), so a size change does
/// not leave a mixed-size column. Guarded on the *step* changing — the
/// settings resource dirties on every unrelated write too.
pub(crate) fn restyle_chat_overlay(
    settings: Option<Res<ViewerSettings>>,
    mut last_step: Local<Option<u32>>,
    mut lines: Query<&mut TextFont, With<ChatOverlayLine>>,
) {
    let step = font_step(settings.as_deref());
    if *last_step == Some(step) {
        return;
    }
    *last_step = Some(step);
    let size = bevy::text::FontSize::Px(overlay_font_size(step));
    for mut font in &mut lines {
        if font.font_size != size {
            font.font_size = size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHAT_FADE_DURATION, CHAT_HOLD_TIME, ChatOverlayContainer, format_chat_line, is_displayable,
        is_faded, line_alpha, setup_chat_overlay,
    };
    use crate::ui::{UiRoot, UiScaffoldSystems};
    use crate::ui_test::{LayoutTest, TestError, find_by_name, settle};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ChatAudible, ChatMessage, ChatSource, ChatType, RegionCoordinates};

    /// Build a minimal received chat message with the given speaker, type, and
    /// text for the formatting tests.
    fn message(from_name: &str, chat_type: ChatType, text: &str) -> ChatMessage {
        ChatMessage {
            from_name: from_name.to_owned(),
            source: ChatSource::System,
            owner_id: None,
            chat_type,
            audible: ChatAudible::Fully,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            message: text.to_owned(),
        }
    }

    /// A normal say is `"{from_name}: {message}"` with no prefix; a whisper and a
    /// shout carry a short volume label.
    #[test]
    fn format_labels_only_whisper_and_shout() {
        assert_eq!(
            format_chat_line(&message("Avatar Tester", ChatType::Normal, "hi")),
            "Avatar Tester: hi"
        );
        assert_eq!(
            format_chat_line(&message("Avatar Tester", ChatType::Whisper, "psst")),
            "[whisper] Avatar Tester: psst"
        );
        assert_eq!(
            format_chat_line(&message("Avatar Tester", ChatType::Shout, "HEY")),
            "[shout] Avatar Tester: HEY"
        );
    }

    /// Speaker classification: the own agent is `Own`, another agent
    /// `Others`, an object `Objects`, and system / unknown sources `System` —
    /// with no identity every agent reads as `Others`.
    #[test]
    fn source_classification_by_speaker() {
        use sl_client_bevy::{AgentKey, ObjectKey, Uuid};

        use super::{ChatLineSource, classify_source};

        let own = AgentKey::from(Uuid::from_u128(1));
        let other = AgentKey::from(Uuid::from_u128(2));
        let mut chat = message("A", ChatType::Normal, "hi");

        chat.source = ChatSource::Agent(own);
        assert_eq!(classify_source(&chat, Some(own)), ChatLineSource::Own);
        assert_eq!(classify_source(&chat, None), ChatLineSource::Others);
        chat.source = ChatSource::Agent(other);
        assert_eq!(classify_source(&chat, Some(own)), ChatLineSource::Others);
        chat.source = ChatSource::Object(ObjectKey::from(Uuid::from_u128(3)));
        assert_eq!(classify_source(&chat, Some(own)), ChatLineSource::Objects);
        chat.source = ChatSource::System;
        assert_eq!(classify_source(&chat, Some(own)), ChatLineSource::System);
        chat.source = ChatSource::Unknown {
            source_type: 9,
            source_id: Uuid::from_u128(4),
        };
        assert_eq!(classify_source(&chat, Some(own)), ChatLineSource::System);
    }

    /// Typing triggers and empty-text messages are not displayed.
    #[test]
    fn typing_and_empty_are_not_displayable() {
        assert!(is_displayable(&message("A", ChatType::Normal, "hi")));
        assert!(!is_displayable(&message("A", ChatType::Normal, "")));
        assert!(!is_displayable(&message("A", ChatType::StartTyping, "")));
        assert!(!is_displayable(&message("A", ChatType::StopTyping, "x")));
    }

    /// A line is fully opaque through its hold time, then ramps linearly to fully
    /// transparent over the fade duration, and is marked faded exactly at the end.
    #[test]
    fn alpha_holds_then_fades_to_zero() {
        // Tolerance for the `f32` comparisons — the restriction lints forbid
        // strict float equality, and these ramps are exact only up to rounding.
        let close = |actual: f32, expected: f32| (actual - expected).abs() < 1e-6;
        assert!(close(line_alpha(0.0, CHAT_HOLD_TIME), 1.0));
        assert!(close(line_alpha(CHAT_HOLD_TIME, CHAT_HOLD_TIME), 1.0));
        // Halfway through the fade → half alpha, and not yet faded.
        let mid = CHAT_HOLD_TIME + CHAT_FADE_DURATION / 2.0;
        assert!(close(line_alpha(mid, CHAT_HOLD_TIME), 0.5));
        assert!(!is_faded(mid, CHAT_HOLD_TIME));
        // At and past the end → fully transparent and marked for despawn.
        let end = CHAT_HOLD_TIME + CHAT_FADE_DURATION;
        assert!(close(line_alpha(end, CHAT_HOLD_TIME), 0.0));
        assert!(is_faded(end, CHAT_HOLD_TIME));
        assert!(close(line_alpha(end + 100.0, CHAT_HOLD_TIME), 0.0));
        // Still holding right at the hold boundary — not fading yet.
        assert!(!is_faded(CHAT_HOLD_TIME, CHAT_HOLD_TIME));
        // A shorter stored hold fades (and finishes) earlier.
        assert!(close(line_alpha(3.5, 2.0), 0.5));
        assert!(is_faded(5.0, 2.0));
    }

    /// The settings-facing resolvers: the font-size steps map to their sizes,
    /// the toast lifetime converts to a hold (lifetime minus the fade, floored
    /// at zero), and a bare world falls back to the reference constants.
    #[test]
    fn display_settings_resolve_with_fallbacks() {
        let close = |actual: f32, expected: f32| (actual - expected).abs() < 1e-6;
        assert!(close(super::overlay_font_size(0), 13.0));
        assert!(close(super::overlay_font_size(1), 15.0));
        assert!(close(super::overlay_font_size(2), 17.0));
        assert!(close(super::overlay_font_size(99), 15.0));
        assert_eq!(super::font_step(None), 1);
        assert!(close(super::hold_time(None), CHAT_HOLD_TIME));
        assert_eq!(super::max_lines(None), 12);

        let store = sl_settings::SettingsStore::new();
        let mut settings = crate::settings::ViewerSettings::from_store_for_test(store);
        crate::preferences_chat::register_settings(&mut settings);
        settings.set(
            sl_settings::Scope::Global,
            crate::preferences_chat::SETTING_NEARBY_TOAST_LIFETIME,
            sl_settings::SettingValue::U32(10),
        );
        settings.set(
            sl_settings::Scope::Global,
            crate::preferences_chat::SETTING_CHAT_MAX_LINES,
            sl_settings::SettingValue::U32(3),
        );
        assert!(close(super::hold_time(Some(&settings)), 7.0));
        assert_eq!(super::max_lines(Some(&settings)), 3);
    }

    /// The overlay container must spawn as a descendant of [`UiRoot`], not as a
    /// separate top-level root — otherwise the snapshot floater's include-UI-off
    /// hide (`Display::None` on `UiRoot`) would not cover it and its transient
    /// lines would leak into a clean-world-view shot
    /// (`viewer-snapshot-chat-overlay-not-hidden`).
    #[test]
    fn overlay_is_parented_under_ui_root() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        app.add_systems(
            Startup,
            setup_chat_overlay.after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);

        let root = app.world().resource::<UiRoot>().0;
        let overlay =
            find_by_name(&mut app, "chat-overlay").ok_or("chat overlay container did not spawn")?;
        // It carries the marker (so `position_chat_overlay` still finds it) and is
        // a child of the single UI root (so the include-UI hide reaches it).
        assert!(app.world().get::<ChatOverlayContainer>(overlay).is_some());
        let parent = app
            .world()
            .get::<ChildOf>(overlay)
            .ok_or("chat overlay has no parent — it is still a top-level root")?;
        assert_eq!(parent.parent(), root);
        Ok(())
    }
}
