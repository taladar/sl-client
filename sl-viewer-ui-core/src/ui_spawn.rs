//! The spawn vocabulary every panel builds its chrome from: a push button, a
//! labelled row, a plain label.
//!
//! These are not widgets — a widget owns state and behaviour, and lives in
//! `sl-viewer-ui-widgets`. These are the three shapes a panel assembles out of
//! bare `bevy_ui` nodes over and over: a box with a label in it that the caller
//! then wires an observer onto, a row that leads with a label and parents the
//! caller's control after it, and a label on its own.
//!
//! # Why they are here rather than in each panel
//!
//! Before this module, `spawn_action_button` existed **twenty times**,
//! `spawn_labeled_row` eight, `spawn_text_button` five and `spawn_label` five,
//! each a near-copy of the next. They diverged only in hardcoded numbers —
//! padding `10/5`, `10/4`, `10/3`, `10/2`, `8/4`, `8/3`, `8/2`, `12/3`; a one-
//! or two-pixel border or none; whether the label carried `Pickable::IGNORE`,
//! and so whether a hover reached the button under it at all. None of that
//! divergence was a decision; it was what each copy happened to be written
//! with. So the shapes live here once, and what genuinely differs between two
//! buttons — its padding, its colours, its skin class — is data the call site
//! passes rather than a copy it maintains.
//!
//! # What stays at the call site
//!
//! Everything that is about *this* button rather than about buttons:
//!
//! - the marker component that says which action it is, and the observer that
//!   runs it: [`spawn_button`] returns the entities, and the caller does
//!   `commands.entity(spawned.button).insert(action).observe(on_action)`;
//! - the colours. They are parameters rather than a look inherited from here —
//!   but since `viewer-ui-button-widget` they are the **pre-load fallback**
//!   rather than the button's appearance: a spec carries a skin class by
//!   default, so `common.css` paints every button and the inline colours are
//!   what a headless harness (which resolves no stylesheet) and the frame
//!   before the sheet lands measure.
//!
//! # The class is a default, not a request
//!
//! It used to be an `Option` that began at `None`, so a button was skinnable
//! only where its panel remembered to chain `.class(…)` — and 14 of the 30
//! `ButtonSpec` call sites never did. That is not a thing a test noticed,
//! because every skin test spawned its own `.sk-button` node rather than asking
//! what the viewer actually spawns; it was found by eye, when the `relief`
//! theme dressed the floater chips in nine-sliced art and left the panel,
//! preferences and debug-settings buttons flat.
//!
//! So [`ButtonSpec::bordered`] starts at [`BUTTON_CLASS`] and
//! [`ButtonSpec::flat`] at [`ACTION_BUTTON_CLASS`], and [`ButtonSpec::class`]
//! is an *override* for a panel that means a different one (the toolbar, the
//! floater chrome). A button that reaches this module is skinned.
//!
//! # The one behaviour this module does decide
//!
//! A helper-spawned label always carries `Pickable::IGNORE`. A label is never a
//! pick target: without it the text node blocks the pointer and the box under
//! it never sees the hover, so the button does not light up when the pointer is
//! over its own caption. Most copies had it; the ones that did not were not
//! choosing differently, they were missing it.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy_flair::style::components::ClassList;

use crate::i18n::Translated;
use crate::skin::{
    ACTION_BUTTON_CLASS, BUTTON_CLASS, COMPACT_BUTTON_CLASS, TEXT_CLASS, role_class,
};
use crate::ui_font::UiFont;

/// What a helper-spawned label says.
///
/// The two are not interchangeable: a [`Self::Key`] label re-resolves itself
/// when the locale changes, a [`Self::Literal`] one cannot and must not — its
/// text came from the grid (a script dialog's own button captions, a group
/// notice's subject) and is already in whatever language the object's author
/// wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiLabel {
    /// A Fluent key, resolved (and re-resolved on a locale switch) by
    /// [`Translated`].
    Key(String),
    /// Text that is already in the user's language and is not translatable.
    Literal(String),
}

impl UiLabel {
    /// A translatable label, named by its Fluent key.
    #[must_use]
    pub fn key(key: impl Into<String>) -> Self {
        Self::Key(key.into())
    }

    /// A label whose text is fixed — it came from the grid, not from a bundle.
    #[must_use]
    pub fn literal(text: impl Into<String>) -> Self {
        Self::Literal(text.into())
    }
}

/// Which button component the spawned box carries.
///
/// Two different `Button` types are in play in `bevy` 0.19 and a panel means
/// one or the other, so the choice is explicit rather than guessed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    /// `bevy_ui`'s [`Button`] marker: requires `Interaction` and a blocking
    /// focus policy, and is what a panel observing `Pointer<Press>` wants.
    #[default]
    Interaction,
    /// `bevy_ui_widgets`' headless button: keeps a pressed state and emits
    /// `Activate` when released, for a panel that observes that instead.
    Headless,
    /// No button component at all — a plain clickable box, which is what the
    /// flat action columns (People, Groups, My Environments) are.
    Plain,
}

/// A button to spawn: what it says, what it is called, and how it is painted.
///
/// Built with [`ButtonSpec::bordered`] or [`ButtonSpec::flat`] and then narrowed
/// by the setters, so a call site names only what differs from the shape it
/// asked for.
#[derive(Clone, Debug)]
pub struct ButtonSpec {
    /// What the button says.
    pub label: UiLabel,
    /// Its [`Name`], which is what a failing UI test and the entity inspector
    /// call it. Panels name theirs `<panel>-button:<key>`.
    pub name: String,
    /// Its tab stop, when it takes keyboard focus. `None` leaves it out of the
    /// tab cycle — which is also what keeps the focus ring off it, since the
    /// skin stamps that off `TabIndex`.
    pub tab_index: Option<i32>,
    /// Which button component it carries.
    pub kind: ButtonKind,
    /// The box's layout: padding, border widths, and any alignment the panel
    /// needs. Everything painted is a field of its own below.
    pub node: Node,
    /// The resting background.
    pub background: Color,
    /// The border colour, painted whether or not `node.border` is non-zero (a
    /// zero-width border simply never shows it).
    pub border_color: Color,
    /// The label's size in logical pixels.
    pub font_size: f32,
    /// The label's colour.
    pub label_color: Color,
    /// The skin class on the box. Defaults to the one the constructor's shape
    /// implies, so a button is skinned without being asked; see the [module
    /// documentation](self).
    pub class: Option<&'static str>,
    /// A modifier class worn *with* [`class`](Self::class), restating only its
    /// geometry — [`COMPACT_BUTTON_CLASS`], set by [`Self::compact`].
    pub modifier: Option<&'static str>,
    /// The skin class on the label, when its colour is skinned separately (the
    /// build tools' value token). Defaults to the role the label's colour
    /// names, and to [`TEXT_CLASS`] for a colour that names none — a button's
    /// caption is chrome, and `:disabled` reaches it through that class.
    pub label_class: Option<&'static str>,
    /// Whether the label refuses to wrap — for a button in a row that must keep
    /// its caption on one line however narrow the row gets.
    pub no_wrap: bool,
    /// Whether the action cannot be taken right now: the box carries
    /// [`InteractionDisabled`], and the skin greys the box and its caption from
    /// `:disabled` rather than the panel choosing a dim colour.
    pub disabled: bool,
}

/// The dominant padding of a bordered push button, in logical pixels.
const BORDERED_PADDING: (f32, f32) = (8.0, 3.0);

/// The dominant padding of a flat action-column button, in logical pixels.
const FLAT_PADDING: (f32, f32) = (8.0, 4.0);

/// The fallback label size, in logical pixels — the size two thirds of the
/// panels use.
const DEFAULT_FONT_SIZE: f32 = 13.0;

impl ButtonSpec {
    /// A bordered push button: a one-pixel frame around `padding`.
    ///
    /// The colours are the neutral panel-button palette; a panel with its own
    /// passes them to [`Self::colors`] and [`Self::label_color`].
    #[must_use]
    pub fn bordered(label: UiLabel, name: impl Into<String>) -> Self {
        Self {
            label,
            name: name.into(),
            tab_index: None,
            kind: ButtonKind::Interaction,
            node: Node {
                padding: UiRect::axes(Val::Px(BORDERED_PADDING.0), Val::Px(BORDERED_PADDING.1)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            background: Color::srgb(0.13, 0.15, 0.20),
            border_color: Color::srgb(0.34, 0.40, 0.52),
            font_size: DEFAULT_FONT_SIZE,
            label_color: Color::srgb(0.90, 0.92, 0.96),
            class: Some(BUTTON_CLASS),
            modifier: None,
            label_class: None,
            no_wrap: false,
            disabled: false,
        }
    }

    /// A flat action-column button: no border, its caption centred, and never
    /// shrinking below it — the shape the People, Groups and My Environments
    /// action columns are built from.
    ///
    /// It carries no button component ([`ButtonKind::Plain`]): those columns
    /// grey a row out rather than disabling it, and answer the press in their
    /// own observer against their own enable predicate.
    #[must_use]
    pub fn flat(label: UiLabel, name: impl Into<String>) -> Self {
        Self {
            node: Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(FLAT_PADDING.0), Val::Px(FLAT_PADDING.1)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            background: Color::srgb(0.24, 0.29, 0.38),
            kind: ButtonKind::Plain,
            class: Some(ACTION_BUTTON_CLASS),
            ..Self::bordered(label, name)
        }
    }

    /// Put it in the tab cycle at `index`.
    #[must_use]
    pub const fn tab_index(mut self, index: i32) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Take the next tab index from `next`, advancing it — the pattern a panel
    /// that spawns a row of buttons in a loop uses.
    #[must_use]
    pub const fn tab_from(self, next: &mut i32) -> Self {
        let index = *next;
        *next = next.saturating_add(1);
        self.tab_index(index)
    }

    /// Set the box's padding, in logical pixels: `inline` on the text axis,
    /// `block` above and below.
    #[must_use]
    pub const fn padding(mut self, inline: f32, block: f32) -> Self {
        self.node.padding = UiRect::axes(Val::Px(inline), Val::Px(block));
        self
    }

    /// Set the border width, in logical pixels.
    #[must_use]
    pub const fn border(mut self, width: f32) -> Self {
        self.node.border = UiRect::all(Val::Px(width));
        self
    }

    /// Paint it: the background, and the colour of whatever border it has.
    #[must_use]
    pub const fn colors(mut self, background: Color, border: Color) -> Self {
        self.background = background;
        self.border_color = border;
        self
    }

    /// Set the label's colour.
    #[must_use]
    pub const fn label_color(mut self, color: Color) -> Self {
        self.label_color = color;
        self
    }

    /// Set the label's size, in logical pixels.
    #[must_use]
    pub const fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Tag the box with a skin class **instead of** the one its shape implies —
    /// for a button whose family is its own (the toolbar, the floater chrome).
    #[must_use]
    pub const fn class(mut self, class: &'static str) -> Self {
        self.class = Some(class);
        self
    }

    /// Spawn it at row scale: the same button, in a table cell or a dense strip
    /// beside a field, wearing [`COMPACT_BUTTON_CLASS`] over its own class.
    ///
    /// Which of the two sizes a button is stays a decision here, because the
    /// call site is the only thing that knows what the button sits in; what
    /// each of them *looks* like is the skin's.
    #[must_use]
    pub const fn compact(mut self) -> Self {
        self.modifier = Some(COMPACT_BUTTON_CLASS);
        self
    }

    /// Refuse it: the box takes [`InteractionDisabled`], and the skin greys it
    /// and its caption.
    ///
    /// Takes the predicate rather than being a bare marker, because every call
    /// site that needs it already has one (`gates.sale`, "the agent may change
    /// this group flag") and would otherwise spell the `if` itself.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Tag the label with a skin class of its own.
    #[must_use]
    pub const fn label_class(mut self, class: &'static str) -> Self {
        self.label_class = Some(class);
        self
    }

    /// Choose the button component it carries.
    #[must_use]
    pub const fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    /// Keep the caption on one line however narrow the button gets.
    #[must_use]
    pub const fn no_wrap(mut self) -> Self {
        self.no_wrap = true;
        self
    }

    /// Adjust the box's layout beyond padding and border — alignment within its
    /// parent, a minimum width, a margin.
    ///
    /// Takes the `Node` the spec has built so far, so a caller edits it rather
    /// than rebuilding the padding and border it just asked for.
    #[must_use]
    pub fn layout(mut self, edit: impl FnOnce(&mut Node)) -> Self {
        edit(&mut self.node);
        self
    }
}

/// What [`spawn_button`] made: the clickable box, and the label node inside it.
///
/// Both, because a panel needs either: the box is what an action component and
/// an observer go on, and the label is what a panel that retitles its button
/// (or greys it) writes to later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnedButton {
    /// The clickable box.
    pub button: Entity,
    /// The label node inside it.
    pub label: Entity,
}

/// Spawn a button under `parent` and return its two entities.
///
/// The caller attaches its own action component and observer to
/// [`SpawnedButton::button`]; this spawns no behaviour of its own.
pub fn spawn_button(commands: &mut Commands, parent: Entity, spec: ButtonSpec) -> SpawnedButton {
    let mut button = commands.spawn((
        spec.node.clone(),
        BackgroundColor(spec.background),
        BorderColor::all(spec.border_color),
        Name::new(spec.name.clone()),
        ChildOf(parent),
    ));
    match spec.kind {
        ButtonKind::Interaction => {
            button.insert(Button);
        }
        ButtonKind::Headless => {
            button.insert(bevy::ui_widgets::Button);
        }
        ButtonKind::Plain => {}
    }
    if let Some(index) = spec.tab_index {
        button.insert(TabIndex(index));
    }
    if spec.disabled {
        button.insert(InteractionDisabled);
    }
    let classes: Vec<&'static str> = spec.class.into_iter().chain(spec.modifier).collect();
    if !classes.is_empty() {
        button.insert(ClassList::new_with_classes(classes));
    }
    let button = button.id();
    // The caption is chrome, so it ends up with a class whatever colour the
    // panel named: its own if the panel gave it one, else the role that colour
    // names, else the plain text class. That last fallback is what
    // `.sk-button:disabled .sk-text` needs to reach — a caption with no class
    // at all is one the refused state cannot grey, which is the shape the
    // hand-rolled buttons were in.
    let label_class = spec
        .label_class
        .or_else(|| role_class(spec.label_color))
        .or(Some(TEXT_CLASS));
    let label = spawn_text(
        commands,
        button,
        &spec.label,
        spec.label_color,
        spec.font_size,
        label_class,
        spec.no_wrap,
    );
    SpawnedButton { button, label }
}

/// A labelled row to spawn: a row that leads with a label and parents the
/// caller's control after it.
#[derive(Clone, Debug)]
pub struct LabeledRowSpec {
    /// The leading label.
    pub label: UiLabel,
    /// Its colour — usually the dim label colour, since the value beside it is
    /// what the reader is after.
    pub label_color: Color,
    /// Its size in logical pixels.
    pub font_size: f32,
    /// A fixed inline size for the label column, when a panel aligns the values
    /// of successive rows against each other.
    pub label_width: Option<Val>,
    /// A lower bound on the label column instead of a fixed width — the same
    /// alignment, but a long translation grows rather than clipping.
    pub label_min_width: Option<Val>,
    /// The gap between the label and the control.
    pub gap: Val,
    /// Whether the row wraps when the control will not fit beside its label.
    pub wrap: bool,
    /// The row's margin.
    pub margin: UiRect,
    /// The row's [`Name`], when the panel names its rows.
    pub name: Option<String>,
}

/// The gap between a labelled row's label and its control, in logical pixels.
const ROW_GAP: f32 = 6.0;

impl LabeledRowSpec {
    /// A labelled row leading with `label`.
    #[must_use]
    pub const fn new(label: UiLabel) -> Self {
        Self {
            label,
            label_color: Color::srgb(0.62, 0.66, 0.74),
            font_size: DEFAULT_FONT_SIZE,
            label_width: None,
            label_min_width: None,
            gap: Val::Px(ROW_GAP),
            wrap: false,
            margin: UiRect::ZERO,
            name: None,
        }
    }

    /// Set the label's colour.
    #[must_use]
    pub const fn label_color(mut self, color: Color) -> Self {
        self.label_color = color;
        self
    }

    /// Set the label's size, in logical pixels.
    #[must_use]
    pub const fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Set the gap between the label and the control.
    #[must_use]
    pub const fn gap(mut self, gap: Val) -> Self {
        self.gap = gap;
        self
    }

    /// Give the label column a fixed inline size.
    #[must_use]
    pub const fn label_width(mut self, width: Val) -> Self {
        self.label_width = Some(width);
        self
    }

    /// Give the label column a lower bound instead of a fixed size.
    #[must_use]
    pub const fn label_min_width(mut self, width: Val) -> Self {
        self.label_min_width = Some(width);
        self
    }

    /// Let the row wrap when its control will not fit beside the label.
    #[must_use]
    pub const fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    /// Set the row's margin.
    #[must_use]
    pub const fn margin(mut self, margin: UiRect) -> Self {
        self.margin = margin;
        self
    }

    /// Name the row.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// What [`spawn_labeled_row`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LabeledRow {
    /// The row, which is what the caller parents its control into.
    pub row: Entity,
    /// The leading label node.
    pub label: Entity,
}

/// Spawn a labelled row under `parent`: the label leading, the caller's content
/// after it.
pub fn spawn_labeled_row(
    commands: &mut Commands,
    parent: Entity,
    spec: LabeledRowSpec,
) -> LabeledRow {
    let mut row = commands.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            // The gap between the items of a row is the *column* gap. The
            // `row_gap` — what separates the lines a wrapping row breaks into —
            // is deliberately left at zero, which is what every panel these
            // rows came from was laid out with.
            column_gap: spec.gap,
            align_items: AlignItems::Center,
            flex_wrap: if spec.wrap {
                FlexWrap::Wrap
            } else {
                FlexWrap::NoWrap
            },
            margin: spec.margin,
            ..default()
        },
        ChildOf(parent),
    ));
    if let Some(name) = spec.name.clone() {
        row.insert(Name::new(name));
    }
    let row = row.id();
    let label = spawn_text(
        commands,
        row,
        &spec.label,
        spec.label_color,
        spec.font_size,
        None,
        false,
    );
    if spec.label_width.is_some() || spec.label_min_width.is_some() {
        commands.entity(label).insert(Node {
            width: spec.label_width.unwrap_or(Val::Auto),
            min_width: spec.label_min_width.unwrap_or(Val::Auto),
            ..default()
        });
    }
    LabeledRow { row, label }
}

/// Spawn a plain label under `parent`, and return it.
///
/// The bare text node a panel puts above a control or beside a value — no box,
/// no decoration. A label with a box around it is a [`spawn_labeled_row`] or a
/// [`spawn_button`].
pub fn spawn_label(
    commands: &mut Commands,
    parent: Entity,
    label: UiLabel,
    color: Color,
    font_size: f32,
) -> Entity {
    spawn_text(commands, parent, &label, color, font_size, None, false)
}

/// Spawn the text node the three helpers all end in.
///
/// One function because the divergence between their labels was never
/// deliberate: every one of them wants the same font role, the same colour
/// handling, and the same `Pickable::IGNORE` (see the module docs).
///
/// The `TextColor` stays beside the class rather than being replaced by it: it
/// is the unskinned value a headless world (and a skin that omits the token)
/// falls back to, and the class beats it wherever a stylesheet is resolved.
fn spawn_text(
    commands: &mut Commands,
    parent: Entity,
    label: &UiLabel,
    color: Color,
    font_size: f32,
    class: Option<&'static str>,
    no_wrap: bool,
) -> Entity {
    let mut text = commands.spawn((
        UiFont::Sans.at(font_size),
        TextColor(color),
        // A label is never a pick target: without this the text blocks the
        // pointer and the box under it never sees the hover.
        Pickable::IGNORE,
        ChildOf(parent),
    ));
    match label {
        UiLabel::Key(key) => {
            text.insert((Text::default(), Translated::new(key.clone())));
        }
        UiLabel::Literal(literal) => {
            text.insert(Text::new(literal.clone()));
        }
    }
    if let Some(class) = class.or_else(|| role_class(color)) {
        text.insert(ClassList::new_with_classes([class]));
    }
    if no_wrap {
        text.insert(TextLayout::no_wrap());
    }
    text.id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::{DISABLED_TEXT_CLASS, HEADING_CLASS, TITLE_CLASS};
    use crate::skin_palette::SkinPalette;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A world with just enough of the UI stack to spawn into.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app
    }

    /// Run one set of commands against `app`'s world, apply them, and hand back
    /// whatever the closure returned.
    fn apply<R>(app: &mut App, spawn: impl FnOnce(&mut Commands) -> R) -> R {
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        let made = spawn(&mut commands);
        queue.apply(app.world_mut());
        made
    }

    /// A bordered button carries the box components, the tab stop and a label
    /// child — and the label is `Pickable::IGNORE`, which is the hover bug the
    /// scattered copies kept reintroducing.
    #[test]
    fn bordered_button_is_a_box_with_an_ignoring_label() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let spawned = apply(&mut app, |commands| {
            spawn_button(
                commands,
                parent,
                ButtonSpec::bordered(UiLabel::key("about-land-buy"), "about-land-button:buy")
                    .tab_index(7)
                    .padding(10.0, 5.0),
            )
        });
        let world = app.world();
        assert!(world.get::<Button>(spawned.button).is_some());
        assert_eq!(
            world.get::<TabIndex>(spawned.button).map(|index| index.0),
            Some(7)
        );
        assert_eq!(
            world.get::<Node>(spawned.button).map(|node| node.padding),
            Some(UiRect::axes(Val::Px(10.0), Val::Px(5.0)))
        );
        assert_eq!(
            world.get::<ChildOf>(spawned.label).map(ChildOf::parent),
            Some(spawned.button)
        );
        assert_eq!(
            world.get::<Pickable>(spawned.label),
            Some(&Pickable::IGNORE)
        );
        assert!(world.get::<Translated>(spawned.label).is_some());
        Ok(())
    }

    /// **A button is skinned without being asked.**
    ///
    /// The whole of `viewer-ui-button-widget`: the class used to be an `Option`
    /// starting at `None`, so a stylesheet reached a button only where its panel
    /// remembered to chain `.class(…)`. Each shape now starts at the class its
    /// family wears, `.class(…)` is an override, and `.compact()` adds the
    /// row-scale modifier beside it.
    ///
    /// The caption is covered too, and by the role its colour names where there
    /// is one — a caption with no class at all is a caption
    /// `.sk-button:disabled .sk-text` cannot grey.
    #[test]
    fn every_shape_of_button_carries_a_class_the_skin_can_select() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let cases = [
            (
                ButtonSpec::bordered(UiLabel::key("about-land-buy"), "bordered"),
                vec![BUTTON_CLASS],
                TEXT_CLASS,
            ),
            (
                ButtonSpec::flat(UiLabel::key("people-im"), "flat"),
                vec![ACTION_BUTTON_CLASS],
                TEXT_CLASS,
            ),
            (
                ButtonSpec::bordered(UiLabel::key("about-land-remove"), "compact").compact(),
                vec![BUTTON_CLASS, COMPACT_BUTTON_CLASS],
                TEXT_CLASS,
            ),
            (
                ButtonSpec::bordered(UiLabel::key("toolbar-chat"), "own-family")
                    .class("sk-toolbar-button"),
                vec!["sk-toolbar-button"],
                TEXT_CLASS,
            ),
            (
                // A caption that names a role keeps it rather than being
                // flattened onto the plain text class.
                ButtonSpec::bordered(UiLabel::key("build-land-apply"), "muted-caption")
                    .label_color(SkinPalette::FALLBACK.text_muted),
                vec![BUTTON_CLASS],
                TITLE_CLASS,
            ),
        ];
        for (spec, wanted_box, wanted_label) in cases {
            let name = spec.name.clone();
            let spawned = apply(&mut app, |commands| spawn_button(commands, parent, spec));
            let world = app.world();
            let classes = world
                .get::<ClassList>(spawned.button)
                .ok_or("a button with no class list at all")?;
            for class in wanted_box {
                assert!(
                    classes.contains(class),
                    "`{name}` must carry `{class}` — without it no stylesheet can reach the box"
                );
            }
            assert!(
                world
                    .get::<ClassList>(spawned.label)
                    .is_some_and(|classes| classes.contains(wanted_label)),
                "`{name}`'s caption must carry `{wanted_label}`, or the refused state cannot \
                 grey it"
            );
        }
        Ok(())
    }

    /// A refused button carries the marker the cascade selects on, and nothing
    /// else says so — the panel does not pick a dim colour of its own.
    #[test]
    fn a_refused_button_carries_the_marker_not_a_dim_colour() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let live = apply(&mut app, |commands| {
            spawn_button(
                commands,
                parent,
                ButtonSpec::bordered(UiLabel::key("item-properties-for-sale"), "live")
                    .disabled(false),
            )
        });
        let refused = apply(&mut app, |commands| {
            spawn_button(
                commands,
                parent,
                ButtonSpec::bordered(UiLabel::key("item-properties-for-sale"), "refused")
                    .disabled(true),
            )
        });
        let world = app.world();
        assert!(world.get::<InteractionDisabled>(live.button).is_none());
        assert!(world.get::<InteractionDisabled>(refused.button).is_some());
        assert_eq!(
            world.get::<TextColor>(live.label).map(|text| text.0),
            world.get::<TextColor>(refused.label).map(|text| text.0),
            "the refusal is a state, not a colour: both captions are painted the same and \
             `.sk-button:disabled .sk-text` is what greys one of them"
        );
        Ok(())
    }

    /// A flat button is the action-column shape: no border, no button
    /// component, centred and never shrinking.
    #[test]
    fn flat_button_is_a_plain_centred_box() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let spawned = apply(&mut app, |commands| {
            spawn_button(
                commands,
                parent,
                ButtonSpec::flat(UiLabel::key("people-im"), "people-friends-action"),
            )
        });
        let world = app.world();
        assert!(world.get::<Button>(spawned.button).is_none());
        assert!(world.get::<TabIndex>(spawned.button).is_none());
        let node = world
            .get::<Node>(spawned.button)
            .ok_or("the spawned button is a node")?;
        assert_eq!(node.border, UiRect::ZERO);
        assert_eq!(node.align_items, AlignItems::Center);
        assert!((node.flex_shrink - 0.0).abs() < f32::EPSILON);
        Ok(())
    }

    /// A literal label is not bound to a Fluent key: text from the grid must
    /// not be re-resolved when the locale changes.
    #[test]
    fn a_literal_label_is_not_translated() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let spawned = apply(&mut app, |commands| {
            spawn_button(
                commands,
                parent,
                ButtonSpec::bordered(UiLabel::literal("Touch me"), "script-dialog-action:touch"),
            )
        });
        let world = app.world();
        assert!(world.get::<Translated>(spawned.label).is_none());
        assert_eq!(
            world.get::<Text>(spawned.label).map(|text| text.0.clone()),
            Some("Touch me".to_owned())
        );
        Ok(())
    }

    /// A labelled row bounds its label column when asked, and wraps only when
    /// asked.
    #[test]
    fn a_labeled_row_bounds_its_label_column() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let row = apply(&mut app, |commands| {
            spawn_labeled_row(
                commands,
                parent,
                LabeledRowSpec::new(UiLabel::key("about-landmark-owner"))
                    .label_min_width(Val::Px(90.0))
                    .wrap(),
            )
        });
        let world = app.world();
        assert_eq!(
            world.get::<Node>(row.row).map(|node| node.flex_wrap),
            Some(FlexWrap::Wrap)
        );
        assert_eq!(
            world.get::<Node>(row.label).map(|node| node.min_width),
            Some(Val::Px(90.0))
        );
        Ok(())
    }

    /// `tab_from` hands out consecutive indices and leaves the counter on the
    /// next free one, which is what a loop over a row of buttons needs.
    #[test]
    fn tab_from_advances_the_counter() {
        let mut tab = 4;
        let first = ButtonSpec::bordered(UiLabel::key("a"), "a").tab_from(&mut tab);
        let second = ButtonSpec::bordered(UiLabel::key("b"), "b").tab_from(&mut tab);
        assert_eq!(first.tab_index, Some(4));
        assert_eq!(second.tab_index, Some(5));
        assert_eq!(tab, 6);
    }

    /// **A label that names a role is skinnable; one that names a colour is
    /// not.**
    ///
    /// This is the whole of `viewer-skin-panel-text-roles`' first move: a panel
    /// asking for `FALLBACK.text_muted` is naming a role, so the label carries
    /// `.sk-title` and a skin recolours it — with no change at the ~230 call
    /// sites. A panel asking for a colour of its own has named no role, gets no
    /// class, and keeps painting itself until someone decides what it meant.
    ///
    /// The `TextColor` stays either way: it is what a headless world, and a
    /// skin that omits the token, fall back to.
    #[test]
    fn a_label_takes_the_class_of_the_role_its_colour_names() -> Result<(), TestError> {
        let mut app = app();
        let parent = app.world_mut().spawn(Node::default()).id();
        let cases = [
            (SkinPalette::FALLBACK.text_primary, Some(TEXT_CLASS)),
            (SkinPalette::FALLBACK.text_muted, Some(TITLE_CLASS)),
            (SkinPalette::FALLBACK.text_heading, Some(HEADING_CLASS)),
            (
                SkinPalette::FALLBACK.text_disabled,
                Some(DISABLED_TEXT_CLASS),
            ),
            // A colour that is nobody's role.
            (Color::srgb(0.13, 0.79, 0.31), None),
        ];
        for (color, wanted) in cases {
            let label = apply(&mut app, |commands| {
                spawn_label(commands, parent, UiLabel::literal("a label"), color, 13.0)
            });
            let world = app.world();
            let classes = world.get::<ClassList>(label);
            match wanted {
                Some(class) => assert!(
                    classes.is_some_and(|classes| classes.contains(class)),
                    "a label coloured with a role must carry that role's class"
                ),
                None => assert!(
                    classes.is_none(),
                    "a label with a colour of its own must not be given a role's class"
                ),
            }
            assert_eq!(
                world.get::<TextColor>(label).map(|text| text.0),
                Some(color),
                "the unskinned colour stays beside the class"
            );
        }
        Ok(())
    }
}
