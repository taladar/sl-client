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
use sl_client_bevy::{ChatMessage, ChatType, SlEvent, SlSessionEvent};

use crate::bottom_toolbar::BottomArea;
use crate::ui_font::UiFont;

/// The most chat lines the overlay ever shows at once. Fading already bounds each
/// line's lifetime; this is the burst safety valve, evicting the oldest line so a
/// flood of near-simultaneous chat cannot grow the column without bound.
const CHAT_MAX_LINES: usize = 12;

/// How long, in seconds, a freshly arrived line stays fully opaque before it
/// begins to fade. Reference-faithful: Firestorm's `NearbyToastLifeTime` (23 s)
/// minus its `NearbyToastFadingTime` (3 s).
const CHAT_HOLD_TIME: f32 = 20.0;

/// How long, in seconds, a line takes to fade from fully opaque to fully
/// transparent once its hold time lapses. Matches Firestorm's
/// `NearbyToastFadingTime`.
const CHAT_FADE_DURATION: f32 = 3.0;

/// The overlay font size, in logical pixels.
const CHAT_FONT_SIZE: f32 = 15.0;

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

/// A line's alpha from its age: fully opaque through [`CHAT_HOLD_TIME`], then a
/// linear ramp down to `0.0` over [`CHAT_FADE_DURATION`], clamped to `[0, 1]`.
fn line_alpha(age: f32) -> f32 {
    if age <= CHAT_HOLD_TIME {
        1.0
    } else {
        let faded = (age - CHAT_HOLD_TIME) / CHAT_FADE_DURATION;
        (1.0 - faded).clamp(0.0, 1.0)
    }
}

/// Whether a line has fully faded and should be despawned.
fn is_faded(age: f32) -> bool {
    age >= CHAT_HOLD_TIME + CHAT_FADE_DURATION
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
pub(crate) fn setup_chat_overlay(mut commands: Commands) {
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
    mut overlay: ResMut<ChatOverlay>,
    container: Query<Entity, With<ChatOverlayContainer>>,
) {
    let Ok(container) = container.single() else {
        return;
    };
    for event in events.read() {
        if let SlSessionEvent::ChatReceived(message) = &event.0
            && is_displayable(message)
        {
            let line = format_chat_line(message);
            debug!("chat overlay: {line}");
            let seq = overlay.next_seq;
            overlay.next_seq = overlay.next_seq.wrapping_add(1);
            commands.spawn((
                Text::new(line),
                UiFont::Sans.at(CHAT_FONT_SIZE),
                TextColor(Color::WHITE),
                // Transparent to picks, like its container: a fading chat line must
                // not block a world click that happens to land on it.
                Pickable::IGNORE,
                ChatOverlayLine { age: 0.0, seq },
                ChildOf(container),
            ));
        }
    }
}

/// Advance every line's age by this frame's delta, drive each line's alpha from
/// its own age, despawn lines that have fully faded, and evict the oldest lines
/// beyond [`CHAT_MAX_LINES`] so a burst cannot grow the column without bound.
pub(crate) fn tick_chat_overlay(
    mut commands: Commands,
    time: Res<Time>,
    mut lines: Query<(Entity, &mut ChatOverlayLine, &mut TextColor)>,
) {
    let dt = time.delta_secs();
    // Surviving (not-yet-faded) lines with their arrival order, for the overflow
    // pass below.
    let mut survivors: Vec<(Entity, u64)> = Vec::new();
    for (entity, mut line, mut color) in &mut lines {
        line.age += dt;
        if is_faded(line.age) {
            commands.entity(entity).despawn();
            continue;
        }
        color.0 = color.0.with_alpha(line_alpha(line.age));
        survivors.push((entity, line.seq));
    }
    if survivors.len() > CHAT_MAX_LINES {
        survivors.sort_unstable_by_key(|&(_, seq)| seq);
        let overflow = survivors.len().saturating_sub(CHAT_MAX_LINES);
        for &(entity, _) in survivors.iter().take(overflow) {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHAT_FADE_DURATION, CHAT_HOLD_TIME, format_chat_line, is_displayable, is_faded, line_alpha,
    };
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
        assert!(close(line_alpha(0.0), 1.0));
        assert!(close(line_alpha(CHAT_HOLD_TIME), 1.0));
        // Halfway through the fade → half alpha, and not yet faded.
        let mid = CHAT_HOLD_TIME + CHAT_FADE_DURATION / 2.0;
        assert!(close(line_alpha(mid), 0.5));
        assert!(!is_faded(mid));
        // At and past the end → fully transparent and marked for despawn.
        let end = CHAT_HOLD_TIME + CHAT_FADE_DURATION;
        assert!(close(line_alpha(end), 0.0));
        assert!(is_faded(end));
        assert!(close(line_alpha(end + 100.0), 0.0));
        // Still holding right at the hold boundary — not fading yet.
        assert!(!is_faded(CHAT_HOLD_TIME));
    }
}
