//! The **emoji-picker floater** (`viewer-emoji-picker-floater`): a grouped,
//! searchable grid of emoji in a floating window; clicking one inserts its glyph
//! into the text field the picker last saw focused.
//!
//! # What it composes
//!
//! Nothing here is a new primitive — the picker is an assembly of widgets that
//! already landed:
//!
//! - the **floater** chrome ([`crate::floater`]) it lives in, toggled with
//!   `Ctrl+E`;
//! - the reusable **search field** ([`crate::ui_search`]) whose term narrows the
//!   grid;
//! - the reusable **tab strip** ([`crate::ui_tab`]) as a row of category icons,
//!   one per Unicode [`Group`], selecting which group the grid shows;
//! - the **virtualized list** ([`crate::virtual_list`]) backing the grid, one
//!   pooled *row of cells* per list row, so the whole ~1800-glyph dataset costs
//!   the viewport, not the item count;
//! - the emoji **data** ([`sl_emoji`]) — the grouped list and the free-text
//!   [`search`](sl_emoji::search) the grid draws from.
//!
//! # How a chosen glyph reaches a field
//!
//! The picker does not own a text field of its own to type into (its search box
//! is for filtering). Instead it remembers, in [`EmojiTarget`], the last
//! [`EditableText`] *outside the picker* to hold focus, and a cell press inserts
//! the chosen glyph there through the field's own [`EditableText::editor`] — at
//! the caret, replacing any selection, grapheme- and IME-correctly, because it is
//! parley doing the edit, not a raw `set_text`. Focusing the search box, a group
//! tab or the grid never disturbs that remembered target.
//!
//! The field-side half — an emoji *button* beside a chat / IM field that opens
//! this picker anchored to it — is its own task
//! ([[viewer-ui-text-input-emoji]](crate)); until it lands, `Ctrl+E` is the
//! opener and "the last focused field" is the target. Recently-used glyphs and
//! the inline `:shortcode:` completer ([[viewer-emoji-colon-autocomplete]]) are
//! likewise separate follow-ups.
//!
//! # Skin tones
//!
//! A row of six [`SkinTone`] swatches re-casts every tone-bearing glyph in the
//! grid (and the one inserted) to the chosen Fitzpatrick tone; a toneless glyph
//! is left as it is. The picker applies **one** tone at a time — the paired tones
//! the two-person glyphs can take are out of scope, exactly as [`sl_emoji`] models
//! it.
//!
//! # Constructible without its wiring
//!
//! Per the registry rule ([`crate::ui_element`]) the picker's novel layout — the
//! emoji cell, the tone-swatch row and the preview line — is registered as a
//! static specimen ([`spawn_emoji_picker_specimen`]) the gallery / harness sweep
//! across every script, size and direction, with the live toggling, filtering and
//! insertion left to [`EmojiPickerPlugin`].
//!
//! Reference (Firestorm, read-only): `llfloateremojipicker`, `llemojidictionary`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus, InputFocusSystems};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::window::PrimaryWindow;
use sl_emoji::{Emoji, Group, SkinTone, search};

use crate::floater::{
    Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterSpec, spawn_floater,
};
use crate::i18n::Translated;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, TextMayClip};
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::virtual_list::{VirtualList, VirtualRow, VirtualViewport, layout_virtual_lists};

/// The stable id of the picker's floater, keying its persisted geometry.
const EMOJI_FLOATER_ID: &str = "emoji-picker";

/// The `Ctrl` chord key that toggles the picker open / closed. The reference
/// viewer opens it from the chat bar's emoji button; until that field-side button
/// lands, this is the opener.
const TOGGLE_KEY: KeyCode = KeyCode::KeyE;

/// How many emoji columns the grid tiles. A **fixed** count (not adaptive to the
/// window width) so the columns line up and the virtualized rows stay a uniform
/// height — the reference likewise draws a fixed-width grid.
const GRID_COLUMNS: usize = 9;

/// One grid cell's side, in logical pixels — also the virtualized list's uniform
/// row height, since a row is one line of cells.
const CELL_SIZE: f32 = 30.0;

/// The scrolling grid viewport's width, in logical pixels: exactly
/// [`GRID_COLUMNS`] cells wide, so the tiled cells fill it with no slack. Kept in
/// step with [`GRID_COLUMNS`] by a unit test.
const VIEWPORT_WIDTH: f32 = CELL_SIZE * 9.0;

/// The scrolling grid viewport's height, in logical pixels — a **definite** height
/// (a scroll viewport is the case the content-sizing convention carves out, like
/// the inventory list): eight cells tall, scrolling past that.
const VIEWPORT_HEIGHT: f32 = CELL_SIZE * 8.0;

/// The emoji glyph's font size inside a cell, in logical pixels — large enough to
/// read, small enough to sit inside a [`CELL_SIZE`] tile.
const CELL_FONT_SIZE: f32 = 20.0;

/// The chrome (title, tabs, preview) font size, in logical pixels.
const CHROME_FONT_SIZE: f32 = 14.0;

/// A first-frame estimate of the picker window's height, in logical pixels, used to
/// place it **upward** (bottom at the anchor) before it has been measured — the
/// exact height ([`apply_emoji_picker_anchor`]) snaps it the next frame. Erring a
/// little high keeps the window fully on screen while it settles.
const ESTIMATED_PICKER_HEIGHT: f32 = 440.0;

/// A cell's hover highlight — a faint white wash, so the glyph under the pointer
/// reads as the one a click would take.
const CELL_HOVER_BACKGROUND: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);

/// A tone swatch's resting border.
const SWATCH_BORDER: Color = Color::srgb(0.30, 0.36, 0.46);

/// A tone swatch's border when it is the selected tone — bright, so the active
/// tone is unmistakable.
const SWATCH_BORDER_ACTIVE: Color = Color::srgb(0.55, 0.78, 1.0);

/// The preview line's text colour.
const PREVIEW_COLOR: Color = Color::srgb(0.82, 0.86, 0.94);

/// The emoji whose skin-tone variants the swatch row samples — a raised hand,
/// which every skin tone renders. Resolved through [`sl_emoji`] so no glyph is
/// hand-authored here.
const SWATCH_SAMPLE_SHORTCODE: &str = "hand";

/// A representative glyph for a [`Group`], drawn on its tab in the category strip
/// — the first of the two examples the group's own documentation lists.
const fn group_icon(group: Group) -> &'static str {
    match group {
        Group::SmileysAndEmotion => "\u{1f600}", // 😀
        Group::PeopleAndBody => "\u{1f44b}",     // 👋
        Group::AnimalsAndNature => "\u{1f436}",  // 🐶
        Group::FoodAndDrink => "\u{1f347}",      // 🍇
        Group::TravelAndPlaces => "\u{1f697}",   // 🚗
        Group::Activities => "\u{26bd}",         // ⚽
        Group::Objects => "\u{1f4a1}",           // 💡
        Group::Symbols => "\u{1f523}",           // 🔣
        Group::Flags => "\u{1f3c1}",             // 🏁
    }
}

/// The plugin that owns the emoji-picker floater: its resources, its one-time
/// spawn (after the scaffold root exists), and the systems that toggle it, track
/// the target field, mirror the search / group selection into the view, recycle
/// the grid rows, and apply the chosen tone.
pub(crate) struct EmojiPickerPlugin;

impl Plugin for EmojiPickerPlugin {
    /// Wire the picker up. The model half (target tracking, search / tab reads,
    /// view rebuild) runs before the generic list recycles its pool; the row
    /// populate and bind run after, so freshly-recycled rows exist to fill this
    /// frame.
    fn build(&self, app: &mut App) {
        app.init_resource::<EmojiPickerState>()
            .init_resource::<EmojiPickerView>()
            .init_resource::<EmojiTarget>()
            .init_resource::<PendingEmojiAnchor>()
            .add_message::<OpenEmojiPicker>()
            .add_systems(
                Startup,
                spawn_emoji_picker.after(UiScaffoldSystems::SpawnRoot),
            )
            // The toggle is a no-op until the floater exists (it reads the UI
            // resource optionally), so registering it here is safe. The anchor
            // refine runs after the open handler, snapping the window to its
            // measured height once it has been laid out.
            .add_systems(
                Update,
                (
                    toggle_emoji_picker,
                    open_emoji_picker_for_field,
                    apply_emoji_picker_anchor,
                )
                    .chain(),
            )
            // Remember the focused field the frame focus settles, before anything
            // reads the target.
            .add_systems(
                Update,
                track_emoji_target.after(InputFocusSystems::Dispatch),
            )
            .add_systems(
                Update,
                (read_emoji_search, bridge_group_tab, rebuild_emoji_view)
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    populate_new_emoji_rows,
                    bind_emoji_rows,
                    apply_tone_highlight,
                )
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The picker's selection state: which group tab is active, the search term, and
/// the chosen skin tone. The single source of truth the grid is rebuilt from.
#[derive(Resource, Debug, Clone)]
pub(crate) struct EmojiPickerState {
    /// The active group, as an index into [`Group::ALL`]. Ignored while
    /// [`query`](Self::query) is non-blank (search spans every group).
    group: usize,
    /// The current search term; blank shows the active group's list.
    query: String,
    /// The skin tone applied to every tone-bearing glyph in the grid and to the
    /// one inserted.
    tone: SkinTone,
}

impl Default for EmojiPickerState {
    /// Start on the first group, with no search term and the default (tone-neutral)
    /// skin tone.
    fn default() -> Self {
        Self {
            group: 0,
            query: String::new(),
            tone: SkinTone::Default,
        }
    }
}

/// The flattened list the grid currently draws, plus the content signature it was
/// built for so a tone-only change does not needlessly rebuild it.
#[derive(Resource, Debug, Clone)]
pub(crate) struct EmojiPickerView {
    /// Every emoji to show, in order, at their **default** tone — the swatch tone
    /// is applied when a cell is rendered, not stored here.
    emoji: Vec<Emoji>,
    /// The group index this list was built for.
    built_group: usize,
    /// The query this list was built for.
    built_query: String,
}

impl Default for EmojiPickerView {
    /// Start empty, with a group signature that no real group index matches, so
    /// the first rebuild always populates the list.
    fn default() -> Self {
        Self {
            emoji: Vec::new(),
            built_group: usize::MAX,
            built_query: String::new(),
        }
    }
}

/// The [`EditableText`] a chosen glyph is inserted into: the last field **outside
/// the picker** to hold focus, or `None` if none has yet. Remembered across a
/// focus change or clear, so opening the picker, clicking its search box or its
/// grid never loses the field the user was typing in.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct EmojiTarget(Option<Entity>);

/// A just-opened picker's anchor point (the emoji button's press location), held
/// until [`apply_emoji_picker_anchor`] has snapped the window to its measured
/// height — so the open-above / open-below choice uses the real size, not just the
/// first-frame estimate. `None` once applied (or when nothing is pending).
#[derive(Resource, Debug, Clone, Copy, Default)]
struct PendingEmojiAnchor(Option<Vec2>);

/// A request to **open the picker for a specific field**, anchored near a point
/// — written by a field's own emoji button ([`crate::chat_input`]). The picker
/// shows itself, targets `field` (so the next glyph lands there rather than in
/// whatever last held focus), moves next to `near`, and raises to the front.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenEmojiPicker {
    /// The field a chosen glyph should be inserted into.
    pub(crate) field: Entity,
    /// Where to anchor the picker, in logical window pixels (the emoji button's
    /// press location) — the picker's top-leading corner is placed here and the
    /// manager's on-screen clamp pulls it back into view if it would overshoot.
    pub(crate) near: Vec2,
}

/// The picker's live entities, published so the systems reach each part without a
/// marker query per part.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct EmojiPickerUi {
    /// The floater root — its [`UiPanelShown`] opens / closes the window.
    panel: Entity,
    /// The scrolling grid viewport (the [`VirtualList`]).
    viewport: Entity,
    /// The search field's [`EditableText`], whose value is the search term.
    search: Entity,
    /// The category [`TabStrip`], whose active tab is the shown group.
    tab_strip: Entity,
    /// The preview line, showing the hovered glyph's name and short-code.
    preview: Entity,
}

/// One grid cell: the emoji it currently shows, or `None` when it is a trailing
/// blank in the last row. Recycled by [`bind_emoji_rows`], read by the cell's own
/// press / hover observers.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct EmojiCell {
    /// The emoji this cell currently presents (at its default tone; the swatch
    /// tone is applied on top), or `None` when the cell is a trailing blank.
    emoji: Option<Emoji>,
    /// The cell's glyph text node, set by [`bind_emoji_rows`].
    glyph: Entity,
}

/// A pooled grid row's cells, in column order, held on the row so
/// [`bind_emoji_rows`] fills them without re-querying the children.
#[derive(Component, Debug, Clone)]
pub(crate) struct EmojiRowCells {
    /// The row's [`GRID_COLUMNS`] cell entities, in column order.
    cells: Vec<Entity>,
}

/// A skin-tone swatch, naming the tone it selects, so its press observer sets that
/// tone and [`apply_tone_highlight`] can outline the selected one.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct EmojiToneSwatch {
    /// The tone this swatch selects.
    tone: SkinTone,
}

// ---------------------------------------------------------------------------
// Pure helpers — the filtering and tone maths the systems and tests share.
// ---------------------------------------------------------------------------

/// The emoji list a group index and query select: the free-text
/// [`search`](sl_emoji::search) across every group when the query is non-blank,
/// otherwise the active group's own list. A group index past the end yields an
/// empty list rather than panicking.
fn build_view(group: usize, query: &str) -> Vec<Emoji> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        Group::ALL
            .get(group)
            .map_or_else(Vec::new, |group| group.emojis().collect())
    } else {
        search(trimmed)
    }
}

/// The glyph an emoji renders at a chosen tone: the toned variant when it takes
/// tones, or the plain glyph otherwise (applying a tone to a toneless emoji is a
/// clean no-op in [`sl_emoji`]).
fn toned_glyph(emoji: Emoji, tone: SkinTone) -> &'static str {
    emoji.with_skin_tone(tone).unwrap_or(emoji).glyph()
}

/// How many virtualized rows a list of `count` emoji needs at [`GRID_COLUMNS`]
/// per row.
const fn row_count(count: usize) -> usize {
    count.div_ceil(GRID_COLUMNS)
}

// ---------------------------------------------------------------------------
// Systems — target, search, group, view
// ---------------------------------------------------------------------------

/// Remember the last [`EditableText`] outside the picker to hold focus, so a
/// chosen glyph has a field to land in.
///
/// Only updates on a real external field gaining focus — the picker's own search
/// box is excluded, and a focus *clear* leaves the last target in place — so the
/// remembered field survives every interaction with the picker itself.
fn track_emoji_target(
    focus: Res<InputFocus>,
    ui: Option<Res<EmojiPickerUi>>,
    editables: Query<(), With<EditableText>>,
    mut target: ResMut<EmojiTarget>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(focused) = focus.get() else {
        return;
    };
    if focused == ui.search || editables.get(focused).is_err() {
        return;
    }
    if target.0 != Some(focused) {
        target.0 = Some(focused);
    }
}

/// Mirror the picker's search-field value into [`EmojiPickerState::query`].
fn read_emoji_search(
    ui: Option<Res<EmojiPickerUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<EmojiPickerState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.search) else {
        return;
    };
    let text = field.value().to_string();
    if text != state.query {
        state.query = text;
    }
}

/// Mirror the category strip's active tab into [`EmojiPickerState::group`].
fn bridge_group_tab(
    ui: Option<Res<EmojiPickerUi>>,
    strips: Query<&TabStrip, Changed<TabStrip>>,
    mut state: ResMut<EmojiPickerState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(strip) = strips.get(ui.tab_strip) else {
        return;
    };
    if strip.active != state.group {
        state.group = strip.active;
    }
}

/// Recompute the grid's flattened list when the **content** (group or query)
/// changes, and keep the list's row count in step, resetting the scroll so a
/// shorter new list is not left scrolled past its end. A tone-only change does not
/// reach here — [`bind_emoji_rows`] re-renders the visible cells for that.
fn rebuild_emoji_view(
    state: Res<EmojiPickerState>,
    ui: Option<Res<EmojiPickerUi>>,
    mut view: ResMut<EmojiPickerView>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    if view.built_group == state.group && view.built_query == state.query {
        return;
    }
    view.emoji = build_view(state.group, &state.query);
    view.built_group = state.group;
    view.built_query.clone_from(&state.query);
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = row_count(view.emoji.len());
        list.scroll_to_top();
    }
}

// ---------------------------------------------------------------------------
// Systems — the recycled grid rows
// ---------------------------------------------------------------------------

/// Build a newly-pooled grid row's cells: a horizontal line of [`GRID_COLUMNS`]
/// fixed-size cells, each with its own press (insert) and hover (preview /
/// highlight) observers, stored on the row for [`bind_emoji_rows`] to fill.
fn populate_new_emoji_rows(
    mut commands: Commands,
    ui: Option<Res<EmojiPickerUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        commands.entity(row_entity).insert((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(CELL_SIZE),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
        ));
        let mut cells = Vec::with_capacity(GRID_COLUMNS);
        for _column in 0..GRID_COLUMNS {
            cells.push(spawn_live_emoji_cell(&mut commands, row_entity));
        }
        commands.entity(row_entity).insert(EmojiRowCells { cells });
    }
}

/// Spawn one live grid cell under `row_entity`: a fixed-size tile with a centred
/// glyph node and the press / hover observers that make it insert and preview.
fn spawn_live_emoji_cell(commands: &mut Commands, row_entity: Entity) -> Entity {
    let glyph = commands
        .spawn((
            Text::new(""),
            UiFont::Sans.at(CELL_FONT_SIZE),
            TextColor(Color::WHITE),
            // A fixed tile clips an over-large glyph by design, like a scroll
            // viewport — declared so the clipping check knows.
            TextMayClip {
                reason: "an emoji grid cell is a fixed tile; a glyph larger than the tile is \
                         clipped to it, as a scroll viewport clips its content",
            },
            Pickable::IGNORE,
        ))
        .id();
    let cell = commands
        .spawn((
            Node {
                width: Val::Px(CELL_SIZE),
                height: Val::Px(CELL_SIZE),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::default(),
            EmojiCell { emoji: None, glyph },
            ChildOf(row_entity),
        ))
        .add_child(glyph)
        .id();

    // The press (insert), hover (preview + highlight) and unhover observers, each
    // capturing this `cell` so it reads the cell's *current* bound emoji — which
    // recycling keeps up to date — rather than a snapshot.
    commands
        .entity(cell)
        .observe(
            move |mut press: On<Pointer<Press>>,
                  cells: Query<&EmojiCell>,
                  target: Res<EmojiTarget>,
                  state: Res<EmojiPickerState>,
                  mut fields: Query<&mut EditableText>,
                  mut font_cx: ResMut<FontCx>,
                  mut layout_cx: ResMut<LayoutCx>| {
                // Consume the press so it does not fall through to the world or an
                // ancestor's dismiss observer.
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                let Ok(&EmojiCell {
                    emoji: Some(emoji), ..
                }) = cells.get(cell)
                else {
                    return;
                };
                let Some(field) = target.0 else {
                    return;
                };
                insert_glyph_into_field(
                    field,
                    toned_glyph(emoji, state.tone),
                    &mut fields,
                    &mut font_cx,
                    &mut layout_cx,
                );
            },
        )
        .observe(
            move |_over: On<Pointer<Over>>,
                  mut cells: Query<(&EmojiCell, &mut BackgroundColor)>,
                  state: Res<EmojiPickerState>,
                  ui: Option<Res<EmojiPickerUi>>,
                  mut texts: Query<&mut Text>| {
                let Ok((&EmojiCell { emoji, .. }, mut background)) = cells.get_mut(cell) else {
                    return;
                };
                let Some(emoji) = emoji else {
                    return;
                };
                if background.0 != CELL_HOVER_BACKGROUND {
                    background.0 = CELL_HOVER_BACKGROUND;
                }
                if let Some(ui) = ui
                    && let Ok(mut text) = texts.get_mut(ui.preview)
                {
                    let shown = preview_text(emoji, state.tone);
                    if text.0 != shown {
                        text.0 = shown;
                    }
                }
            },
        )
        .observe(
            move |_out: On<Pointer<Out>>, mut backgrounds: Query<&mut BackgroundColor>| {
                if let Ok(mut background) = backgrounds.get_mut(cell)
                    && background.0 != Color::NONE
                {
                    background.0 = Color::NONE;
                }
            },
        );
    cell
}

/// The preview line's text for an emoji at a tone: its toned glyph, its CLDR name,
/// and its primary short-code (when it has one).
fn preview_text(emoji: Emoji, tone: SkinTone) -> String {
    match emoji.shortcode() {
        Some(shortcode) => format!(
            "{}  {}  :{shortcode}:",
            toned_glyph(emoji, tone),
            emoji.name()
        ),
        None => format!("{}  {}", toned_glyph(emoji, tone), emoji.name()),
    }
}

/// Insert `glyph` into `field` at its caret, replacing any selection and leaving
/// the caret after it — the grapheme- and IME-correct edit path
/// ([`EditableText::editor`]'s parley driver), not a raw `set_text`.
fn insert_glyph_into_field(
    field: Entity,
    glyph: &str,
    fields: &mut Query<&mut EditableText>,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) {
    let Ok(mut editable) = fields.get_mut(field) else {
        return;
    };
    insert_glyph_into_editable(&mut editable, glyph, font_cx, layout_cx);
}

/// Insert `glyph` into an [`EditableText`] at its caret — the per-field body of
/// [`insert_glyph_into_field`], split out so a headless test can drive the real
/// parley edit without a [`Query`].
fn insert_glyph_into_editable(
    editable: &mut EditableText,
    glyph: &str,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) {
    let mut driver = editable.editor.driver(font_cx, layout_cx);
    driver.refresh_layout();
    driver.insert_or_replace_selection(glyph);
}

/// Fill each visible grid row's cells from the current view, re-rendering when the
/// view changed (a new group / query) or the state changed (a new tone) or the row
/// was just recycled to a different window index.
fn bind_emoji_rows(
    view: Res<EmojiPickerView>,
    state: Res<EmojiPickerState>,
    ui: Option<Res<EmojiPickerUi>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &EmojiRowCells)>,
    mut cells: Query<&mut EmojiCell>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let rebuild_all = view.is_changed() || state.is_changed();
    for (row, child_of, parts) in &rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !rebuild_all && !row.is_changed() {
            continue;
        }
        let base = row.index.unwrap_or(usize::MAX).saturating_mul(GRID_COLUMNS);
        for (column_index, &cell_entity) in parts.cells.iter().enumerate() {
            let emoji = row
                .index
                .and_then(|_| view.emoji.get(base.saturating_add(column_index)).copied());
            let Ok(mut cell) = cells.get_mut(cell_entity) else {
                continue;
            };
            if cell.emoji != emoji {
                cell.emoji = emoji;
            }
            if let Ok(mut text) = texts.get_mut(cell.glyph) {
                let glyph = emoji.map_or("", |emoji| toned_glyph(emoji, state.tone));
                if text.0 != glyph {
                    glyph.clone_into(&mut text.0);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Systems — skin tone
// ---------------------------------------------------------------------------

/// Outline the swatch of the currently-selected tone, and only that one, so the
/// active skin tone is unmistakable. Writes only on a real change.
fn apply_tone_highlight(
    state: Res<EmojiPickerState>,
    mut swatches: Query<(&EmojiToneSwatch, &mut BorderColor)>,
) {
    if !state.is_changed() {
        return;
    }
    for (swatch, mut border) in &mut swatches {
        let wanted = if swatch.tone == state.tone {
            SWATCH_BORDER_ACTIVE
        } else {
            SWATCH_BORDER
        };
        let wanted = BorderColor::all(wanted);
        if *border != wanted {
            *border = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Toggle
// ---------------------------------------------------------------------------

/// Toggle the picker open / closed on `Ctrl+E`. The `Ctrl` keeps it from firing
/// while a bare `e` is typed into a field.
fn toggle_emoji_picker(
    keyboard: Res<ButtonInput<KeyCode>>,
    ui: Option<Res<EmojiPickerUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(TOGGLE_KEY)) {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = !shown.0;
    }
}

/// Open the picker for a specific field ([`OpenEmojiPicker`]): target that field,
/// anchor the window next to the request point, show it and raise it. Drives the
/// field-side emoji button ([`crate::chat_input`]).
fn open_emoji_picker_for_field(
    mut requests: MessageReader<OpenEmojiPicker>,
    ui: Option<Res<EmojiPickerUi>>,
    mut target: ResMut<EmojiTarget>,
    mut pending: ResMut<PendingEmojiAnchor>,
    mut floaters: Query<(&mut Floater, &mut UiPanelShown)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut commands: MessageWriter<FloaterCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    // Only the last request in a frame matters (two buttons cannot both win).
    let Some(request) = requests.read().last() else {
        return;
    };
    let anchor = request.near;
    target.0 = Some(request.field);
    let viewport_height = windows.single().map_or(f32::MAX, Window::height);
    if let Ok((mut floater, mut shown)) = floaters.get_mut(ui.panel) {
        // Place it with a first-frame estimate (bottom-at-anchor or top-at-anchor,
        // whichever fits); `apply_emoji_picker_anchor` snaps it to the measured
        // height next frame.
        let top = anchor_top(anchor.y, ESTIMATED_PICKER_HEIGHT, viewport_height);
        floater.set_position(Vec2::new(anchor.x, top));
        shown.0 = true;
    }
    pending.0 = Some(anchor);
    commands.write(FloaterCommand {
        floater: ui.panel,
        op: FloaterOp::BringToFront,
    });
}

/// The picker window's top edge for an anchor, opening in whichever direction has
/// room for the whole window: **below** the anchor (top at the anchor) when it
/// fits, otherwise **above** (bottom at the anchor). Never negative, so the top
/// never leaves the screen.
fn anchor_top(anchor_y: f32, height: f32, viewport_height: f32) -> f32 {
    if anchor_y + height <= viewport_height {
        anchor_y
    } else {
        (anchor_y - height).max(0.0)
    }
}

/// Snap the just-opened picker to its **measured** height once it has been laid out
/// (a frame after [`open_emoji_picker_for_field`] showed it), re-choosing the
/// open-above / open-below direction from the real size so the whole window fits.
fn apply_emoji_picker_anchor(
    ui: Option<Res<EmojiPickerUi>>,
    mut pending: ResMut<PendingEmojiAnchor>,
    mut floaters: Query<(&mut Floater, &UiPanelShown)>,
    computed: Query<&ComputedNode>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(anchor) = pending.0 else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    let Ok((mut floater, shown)) = floaters.get_mut(ui.panel) else {
        pending.0 = None;
        return;
    };
    if !shown.0 {
        // Closed again before it was measured — nothing to place.
        pending.0 = None;
        return;
    }
    let Ok(node) = computed.get(ui.panel) else {
        return;
    };
    let height = node.size().y * node.inverse_scale_factor();
    if height <= 0.0 {
        // Not laid out yet — try again next frame.
        return;
    }
    let viewport_height = windows.single().map_or(f32::MAX, Window::height);
    let top = anchor_top(anchor.y, height, viewport_height);
    floater.set_position(Vec2::new(anchor.x, top));
    pending.0 = None;
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// Startup: spawn the picker's floater and its content — the search field, the
/// category strip, the scrolling grid, the tone-swatch row and the preview line —
/// and publish [`EmojiPickerUi`]. Registers [`toggle_emoji_picker`] here (rather
/// than in `build`) so the toggle exists exactly when the floater does.
fn spawn_emoji_picker(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: EMOJI_FLOATER_ID,
            title: "Emoji".to_owned(),
            position: Vec2::new(360.0, 120.0),
            // A fixed, content-driven palette: the grid viewport carries the
            // definite size, so the window sizes to it rather than taking a
            // resizable rect.
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: true,
                closable: true,
                dockable: true,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("emoji-picker-title"));

    let (viewport, search, tab_strip, preview) = build_emoji_picker_content(&mut commands, handle);

    commands.insert_resource(EmojiPickerUi {
        panel: handle.root,
        viewport,
        search,
        tab_strip,
        preview,
    });
}

/// Build the picker's content into the floater's content slot, returning the
/// viewport, search field, category strip and preview line the plugin needs.
fn build_emoji_picker_content(
    commands: &mut Commands,
    handle: FloaterHandle,
) -> (Entity, Entity, Entity, Entity) {
    let content = handle.content;

    // Search field — the reusable widget (`crate::ui_search`), the same box the
    // menu bar and inventory use. Its term drives the grid via `read_emoji_search`.
    let search = spawn_search_field(
        commands,
        content,
        &SearchFieldSpec {
            tab_index: 1,
            font_size: CHROME_FONT_SIZE,
            placeholder: "Search emoji".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("emoji-picker")
        },
    )
    .field;

    // Category strip — the reusable tab widget as a row of group-icon tabs. Its
    // active tab selects the group (`bridge_group_tab`); labels are glyphs, so not
    // translated.
    let labels: Vec<String> = Group::ALL
        .into_iter()
        .map(|group| group_icon(group).to_owned())
        .collect();
    let tab_strip = spawn_tab_strip(
        commands,
        content,
        &TabSpec {
            element: "emoji-picker-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 2,
            font_size: CHROME_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );

    // The scrolling grid viewport — a definite-size `VirtualList` the pooled rows
    // of cells live in. Focusable (so the wheel scrolls it once clicked into) and
    // clipping (so cells past the last visible line are cut at the edge).
    let viewport = commands
        .spawn((
            Node {
                width: Val::Px(VIEWPORT_WIDTH),
                height: Val::Px(VIEWPORT_HEIGHT),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            VirtualList::new(CELL_SIZE),
            VirtualViewport,
            Pickable::default(),
            TabIndex(3),
            Name::new("emoji-picker-viewport"),
            ChildOf(content),
        ))
        // Focus the viewport on a press so the wheel scrolls it rather than
        // zooming the camera (the input-context gate). The remembered target field
        // is unaffected — the viewport is not an `EditableText`.
        .observe(|press: On<Pointer<Press>>, mut focus: ResMut<InputFocus>| {
            if press.button == PointerButton::Primary {
                focus.set(press.entity, FocusCause::Navigated);
            }
        })
        .id();

    // The skin-tone swatch row.
    build_tone_row(commands, content);

    // The preview line — the hovered glyph's name and short-code.
    let preview = commands
        .spawn((
            Text::new(""),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(PREVIEW_COLOR),
            TextMayClip {
                reason: "the emoji preview line is a status hint, cut to the window width like a \
                         scrolling field's text rather than widening the window",
            },
            Node {
                max_width: Val::Px(VIEWPORT_WIDTH),
                overflow: Overflow::clip(),
                ..default()
            },
            Name::new("emoji-picker-preview"),
            ChildOf(content),
        ))
        .id();

    (viewport, search, tab_strip, preview)
}

/// Build the six-swatch skin-tone row under `parent`, each swatch a live button
/// that selects its tone.
fn build_tone_row(commands: &mut Commands, parent: Entity) {
    let base = sl_emoji::by_shortcode(SWATCH_SAMPLE_SHORTCODE);
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(3.0))
            },
            Name::new("emoji-picker-tones"),
            ChildOf(parent),
        ))
        .id();
    for tone in SkinTone::ALL {
        let swatch = spawn_tone_swatch(commands, row_entity, base, tone);
        commands.entity(swatch).observe(
            move |mut press: On<Pointer<Press>>, mut state: ResMut<EmojiPickerState>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                if state.tone != tone {
                    state.tone = tone;
                }
            },
        );
    }
}

/// Spawn one tone swatch under `parent`: a bordered tile showing the sample glyph
/// at `tone`, tagged with the tone it selects. `base` is the tone-sample emoji, or
/// `None` if it could not be resolved (the swatch then shows a bare box).
fn spawn_tone_swatch(
    commands: &mut Commands,
    parent: Entity,
    base: Option<Emoji>,
    tone: SkinTone,
) -> Entity {
    let border = if tone == SkinTone::Default {
        SWATCH_BORDER_ACTIVE
    } else {
        SWATCH_BORDER
    };
    let glyph = base.map_or("", |emoji| toned_glyph(emoji, tone));
    commands
        .spawn((
            Node {
                min_width: Val::Px(CELL_SIZE),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(2.0)),
                padding: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(border),
            BackgroundColor(Color::NONE),
            Pickable::default(),
            EmojiToneSwatch { tone },
            Name::new("emoji-picker-tone"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(glyph.to_owned()),
            UiFont::Sans.at(CELL_FONT_SIZE),
            TextColor(Color::WHITE),
            Pickable::IGNORE,
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Registry specimen
// ---------------------------------------------------------------------------

/// Spawn a **static** emoji-picker specimen for the gallery / harness: the
/// picker's novel layout — a couple of grid rows of real glyphs, the tone-swatch
/// row and the preview line — laid out in flow with no live behaviour, so its
/// layout is swept across every script, size and direction.
///
/// The search field and the category strip are swept by their own specimens, so
/// they are not duplicated here. The grid cells are content-sized (padded around
/// the glyph) rather than the live grid's fixed tiles, so a large font grows them
/// rather than being clipped by the containment check.
pub(crate) fn spawn_emoji_picker_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let panel = commands
        .spawn((
            Node {
                ..column(Val::Px(6.0))
            },
            Name::new("emoji-picker-sample"),
            ChildOf(parent),
        ))
        .id();

    // Two sample rows of the first glyphs, content-sized so the sweep grows them.
    let sample: Vec<Emoji> = Group::SmileysAndEmotion
        .emojis()
        .take(GRID_COLUMNS.saturating_mul(2))
        .collect();
    for chunk in sample.chunks(GRID_COLUMNS) {
        let grid_row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..row(Val::Px(2.0))
                },
                ChildOf(panel),
            ))
            .id();
        for emoji in chunk {
            commands
                .spawn((
                    Node {
                        min_width: Val::Px(CELL_SIZE),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        padding: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    Name::new("emoji-picker-sample-cell"),
                    ChildOf(grid_row),
                ))
                .with_child((
                    Text::new(emoji.glyph().to_owned()),
                    cx.font(UiFont::Sans),
                    TextColor(Color::WHITE),
                ));
        }
    }

    // The tone-swatch row (static — no press observers), and a preview line whose
    // prose comes from the sweep sample.
    let base = sl_emoji::by_shortcode(SWATCH_SAMPLE_SHORTCODE);
    let tone_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(3.0))
            },
            ChildOf(panel),
        ))
        .id();
    for tone in SkinTone::ALL {
        spawn_tone_swatch(commands, tone_row, base, tone);
    }

    commands.spawn((
        Text::new(cx.text("Hover an emoji to preview it")),
        cx.font(UiFont::Sans),
        TextColor(PREVIEW_COLOR),
        Name::new("emoji-picker-sample-preview"),
        ChildOf(panel),
    ));

    panel
}

#[cfg(test)]
mod tests {
    use super::{
        CELL_SIZE, EmojiPickerState, GRID_COLUMNS, VIEWPORT_WIDTH, anchor_top, build_view,
        insert_glyph_into_editable, preview_text, row_count, toned_glyph,
    };
    use bevy::text::{EditableText, FontCx, LayoutCx};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_emoji::{Group, SkinTone, by_glyph, by_shortcode};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The viewport is exactly [`GRID_COLUMNS`] cells wide, so the tiled cells
    /// fill it with no slack. The three constants are pinned to their literals
    /// (30 × 9 = 270) so changing one without the others trips here.
    #[expect(
        clippy::float_cmp,
        reason = "the cell and viewport sizes are exact, representable literals, asserted exactly"
    )]
    #[test]
    fn viewport_width_is_columns_of_cells() {
        assert_eq!(CELL_SIZE, 30.0);
        assert_eq!(GRID_COLUMNS, 9);
        assert_eq!(VIEWPORT_WIDTH, 270.0);
    }

    /// A blank query shows the active group's own list; a non-blank query shows
    /// the free-text search across every group.
    #[test]
    fn build_view_switches_between_group_and_search() -> Result<(), TestError> {
        // Blank query, first group: exactly that group's emoji, in order.
        let group0 = build_view(0, "");
        let expected: Vec<_> = Group::SmileysAndEmotion.emojis().collect();
        assert_eq!(group0, expected);
        assert!(
            group0
                .iter()
                .all(|emoji| emoji.group() == Group::SmileysAndEmotion)
        );

        // A search term crosses groups and ranks the exact short-code first.
        let hits = build_view(0, "rocket");
        assert_eq!(hits.first().map(|emoji| emoji.glyph()), Some("🚀"));

        // Whitespace-only is treated as blank (shows the group, not a search).
        assert_eq!(build_view(0, "   "), expected);
        // A group index past the end is an empty list, not a panic.
        assert!(build_view(usize::MAX, "").is_empty());
        Ok(())
    }

    /// A tone re-casts a tone-bearing glyph and leaves a toneless one alone.
    #[test]
    fn toned_glyph_applies_only_to_tone_bearers() -> Result<(), TestError> {
        let wave = by_shortcode("wave").ok_or("no :wave:")?;
        assert_ne!(toned_glyph(wave, SkinTone::Dark), wave.glyph());
        assert_eq!(toned_glyph(wave, SkinTone::Default), wave.glyph());

        let rocket = by_glyph("🚀").ok_or("no 🚀")?;
        assert_eq!(toned_glyph(rocket, SkinTone::Dark), rocket.glyph());
        Ok(())
    }

    /// The row count is the ceiling of the emoji count over the column count, so
    /// a trailing partial row still gets a list row.
    #[test]
    fn row_count_ceils_over_columns() {
        assert_eq!(row_count(0), 0);
        assert_eq!(row_count(1), 1);
        assert_eq!(row_count(GRID_COLUMNS), 1);
        assert_eq!(row_count(GRID_COLUMNS.saturating_add(1)), 2);
    }

    /// The preview line carries the toned glyph, the name and the short-code, and
    /// drops the short-code for an emoji that has none.
    #[test]
    fn preview_text_names_and_codes_the_emoji() -> Result<(), TestError> {
        let rocket = by_glyph("🚀").ok_or("no 🚀")?;
        assert_eq!(
            preview_text(rocket, SkinTone::Default),
            "🚀  rocket  :rocket:"
        );
        Ok(())
    }

    /// The picker opens **below** an anchor when the window fits under it, and
    /// **above** (bottom at the anchor) when it does not — never off the top.
    #[expect(
        clippy::float_cmp,
        reason = "the anchor arithmetic produces exact, representable results, asserted exactly"
    )]
    #[test]
    fn anchor_opens_where_there_is_room() {
        // Room below (anchor high on a tall viewport): top sits at the anchor.
        assert_eq!(anchor_top(100.0, 400.0, 1000.0), 100.0);
        // No room below (anchor near the bottom): open above, bottom at the anchor.
        assert_eq!(anchor_top(900.0, 400.0, 1000.0), 500.0);
        // Taller than the whole viewport: clamped to the top, never negative.
        assert_eq!(anchor_top(300.0, 800.0, 500.0), 0.0);
    }

    /// The default picker state is the first group, no query, the neutral tone.
    #[test]
    fn default_state_is_first_group_no_query_neutral_tone() {
        let state = EmojiPickerState::default();
        assert_eq!(state.group, 0);
        assert!(state.query.is_empty());
        assert_eq!(state.tone, SkinTone::Default);
    }

    /// Inserting a glyph drops it into the field at the caret, through the real
    /// parley editor — the live insert path a cell press drives, exercised here
    /// where the headless harness does not reach.
    #[test]
    fn insert_drops_the_glyph_at_the_caret() {
        let mut font_cx = FontCx::default();
        let mut layout_cx = LayoutCx::default();
        let mut editable = EditableText::new("ab");
        // Put the caret at the end, then insert a glyph through the helper.
        {
            let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
            driver.refresh_layout();
            driver.move_to_text_end();
        }
        insert_glyph_into_editable(&mut editable, "🚀", &mut font_cx, &mut layout_cx);
        assert_eq!(editable.value().to_string(), "ab🚀");
    }
}
