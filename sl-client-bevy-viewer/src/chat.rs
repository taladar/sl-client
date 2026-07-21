//! On-screen local-chat overlay: a `bevy_ui` text node pinned to the
//! bottom-left corner that shows the last few lines of nearby chat.
//!
//! This is the Phase 11 slice — a read-only overlay, no input box. Each
//! [`ChatReceived`](sl_client_bevy::SlSessionEvent::ChatReceived) message
//! (`ChatFromSimulator`: a nearby agent or object speaking) is formatted as
//! `"{from_name}: {message}"` and appended to a bounded, bottom-up history of
//! the last [`CHAT_HISTORY_LINES`] lines. A whisper or a shout carries a short
//! prefix label so the volume is distinguishable; a normal say has none. The
//! node is anchored at the bottom of the screen and grows upward, so the newest
//! line sits at the bottom.
//!
//! The overlay needs no name resolution — the simulator already supplies the
//! speaker's display name in [`ChatMessage::from_name`](sl_client_bevy::ChatMessage) —
//! and no per-message entities: it is one persistent text node whose string is
//! rebuilt whenever a line arrives.

use std::collections::VecDeque;

use bevy::prelude::*;
use sl_client_bevy::{ChatMessage, ChatType, SlEvent, SlSessionEvent};

use crate::bottom_toolbar::BottomArea;
use crate::ui_font::UiFont;

/// How many chat lines to keep in the overlay (older lines scroll off the top).
const CHAT_HISTORY_LINES: usize = 12;

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

/// A marker component tagging the single overlay text node, so the update system
/// can find and rewrite it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ChatOverlayText;

/// The rolling local-chat history backing the overlay: the last
/// [`CHAT_HISTORY_LINES`] formatted lines, oldest first.
#[derive(Resource, Default)]
pub(crate) struct ChatOverlay {
    /// The formatted lines currently shown, oldest at the front (top) and newest
    /// at the back (bottom).
    lines: VecDeque<String>,
}

impl ChatOverlay {
    /// Append a formatted chat line, evicting the oldest line once the history is
    /// full, and return the joined multi-line string to render.
    fn push(&mut self, line: String) -> String {
        self.lines.push_back(line);
        while self.lines.len() > CHAT_HISTORY_LINES {
            self.lines.pop_front();
        }
        self.rendered()
    }

    /// The current history joined newest-at-bottom into one newline-separated
    /// string.
    fn rendered(&self) -> String {
        let mut joined = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index != 0 {
                joined.push('\n');
            }
            joined.push_str(line);
        }
        joined
    }
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

/// Startup system: spawn the persistent overlay text node, pinned to the
/// bottom-left corner. It starts empty and is rewritten as chat arrives.
pub(crate) fn setup_chat_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(CHAT_FONT_SIZE),
        TextColor(Color::WHITE),
        // Anchored at the bottom-left; the node grows upward as lines are added,
        // so the newest line stays at the bottom.
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(CHAT_INSET),
            bottom: Val::Px(CHAT_BOTTOM_INSET),
            ..default()
        },
        ChatOverlayText,
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
    mut overlays: Query<&mut Node, With<ChatOverlayText>>,
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

/// Fold every received local-chat message into the rolling history and rewrite
/// the overlay text node whenever a displayable line arrives.
pub(crate) fn update_chat_overlay(
    mut events: MessageReader<SlEvent>,
    mut overlay: ResMut<ChatOverlay>,
    mut texts: Query<&mut Text, With<ChatOverlayText>>,
) {
    let mut rendered: Option<String> = None;
    for event in events.read() {
        if let SlSessionEvent::ChatReceived(message) = &event.0
            && is_displayable(message)
        {
            let line = format_chat_line(message);
            debug!("chat overlay: {line}");
            rendered = Some(overlay.push(line));
        }
    }
    if let Some(rendered) = rendered
        && let Ok(mut text) = texts.single_mut()
    {
        *text = Text::new(rendered);
    }
}

#[cfg(test)]
mod tests {
    use super::{CHAT_HISTORY_LINES, ChatOverlay, format_chat_line, is_displayable};
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

    /// The history keeps only the last [`CHAT_HISTORY_LINES`] lines, newest at the
    /// bottom of the rendered block.
    #[test]
    fn history_is_bounded_and_bottom_up() {
        let mut overlay = ChatOverlay::default();
        let mut rendered = String::new();
        for index in 0..(CHAT_HISTORY_LINES + 3) {
            rendered = overlay.push(format!("line {index}"));
        }
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), CHAT_HISTORY_LINES);
        // The three oldest lines have scrolled off; the newest is last.
        assert_eq!(lines.first(), Some(&"line 3"));
        assert_eq!(
            lines.last(),
            Some(&format!("line {}", CHAT_HISTORY_LINES + 2).as_str())
        );
    }
}
