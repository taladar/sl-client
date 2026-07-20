//! The line-based menu widget (`viewer-ui-context-menu`) and the reusable menu
//! bar built on it (`viewer-ui-menu-bar`): the conventional pull-down / pop-up
//! menu — a vertical list of entries, some of which check, disable, separate or
//! open a submenu — and a horizontal strip of buttons that each drop one of
//! those lists down.
//!
//! # The other half of the pie
//!
//! [`crate::pie_menu`] is the *radial* presentation of a menu; this is the
//! *line* presentation. The reference viewer makes pie-vs-line a **preference**
//! (`UsePieMenu`), not two feature sets, and the two widgets are two drawings of
//! the same thing: a **tree of entries**, each a label, an action, and the
//! conditions under which it is available and checked. So the entry vocabulary
//! here mirrors the pie's ([`crate::pie_menu::PieAction`] — a `label`, an
//! `action` string, and a named `when` condition), and both widgets dispatch the
//! same way, by writing a [`UiAction`] that someone else routes (the registry
//! rule, [`crate::ui_element`]). What a given domain menu *contains* is
//! per-domain and not here, exactly as it is not in the pie.
//!
//! # Self-managed, on `bevy_ui_widgets`' `Popover`
//!
//! The one upstream piece this leans on is [`Popover`] — edge-flipping
//! placement. Everything else is **driven here**, off pointer-**press**
//! observers on the button / entry rows, rather than through
//! `bevy_ui_widgets`' `Button` / `MenuButton` activation. That indirection
//! (`Pointer<Press>` → `Activate` → `MenuEvent`) proved not to fire in this app,
//! whereas a plain press observer on the row is reliable — so a bar button's
//! press toggles its menu ([`toggle_host`]), an entry's press runs it and closes
//! the stack, and a press that reaches the UI root (i.e. landed on nothing in a
//! menu, because a menu row stops its own press) dismisses everything
//! ([`dismiss_menus_on_press`]). The highlight is painted by
//! [`highlight_menu_hover`], not bevy_flair `:hover`, so it reads identically in
//! the gallery and the viewer.
//!
//! Two consequences worth stating: a child label must be `Pickable::IGNORE`, or
//! it swallows the press and the row never sees it (a child node blocks picking
//! by default); and keyboard traversal of an *open* menu (arrow keys between
//! entries) is a deliberate follow-up — a bar button is `Tab`-reachable, but
//! stepping through the open list is mouse-only for now.
//!
//! # One widget, two containers — why the inventory shares it
//!
//! The reference draws its item rows identically whether they sit in the main
//! menu bar, a right-click context menu, or a gear-button drop-down on a floater.
//! Only the **container** differs, so this exposes the pieces separately:
//!
//! - [`spawn_menu_button`] — one button that drops a [`MenuDef`] beneath itself.
//!   What the inventory window's gear / view buttons want.
//! - [`spawn_menu_bar`] — a horizontal strip of those buttons. The top menu bar.
//! - [`OpenContextMenu`] — pop a [`MenuDef`] at a screen point, with no anchor.
//!
//! All three build the same list ([`build_menu_popup`]) from the same
//! [`MenuDef`].
//!
//! # Direction-neutral (convention 1)
//!
//! Nothing here says `left` / `right`: a menu drops toward the **block end** and
//! a submenu toward the **inline end**, flipping at the screen edge — mirrored
//! under RTL with no separate code, because [`Popover`]'s candidate placements
//! are built from logical drops folded against the live [`UiDirection`].
//!
//! Reference (Firestorm, read-only): `indra/llui/llmenugl.{h,cpp}`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::HoverMap;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use crate::ui::{LogicalMargin, LogicalRect, UiDirection, UiRoot, UiScaffoldSystems, column};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;

// ---------------------------------------------------------------------------
// The declaration. A menu is a tree of entries, authored as data.
// ---------------------------------------------------------------------------

/// One command a menu can run, at one line.
///
/// The shape mirrors [`crate::pie_menu::PieAction`] on purpose — a `label`, an
/// `action` string emitted when picked, and named condition keys — so the two
/// presentations of a menu can share a domain's entries rather than drifting.
/// The extra fields are the ones a *line* has room for that a pie slice does
/// not: an accelerator drawn against the entry, and separate enable / check /
/// visible conditions (the reference's `on_enable` / `on_check` / `on_visible`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MenuCommand {
    /// The entry's text. Laid out through the ordinary bidi text stack.
    pub(crate) label: &'static str,
    /// What this emits when picked — the `action` of the [`UiAction`] the widget
    /// writes, and the name a test asserts against.
    pub(crate) action: &'static str,
    /// The condition under which the entry is **enabled**, or `None` for always.
    /// A failing condition greys the entry and makes it unpickable; it keeps its
    /// line, because the entry belongs to the menu, not to whether it is
    /// available this second.
    pub(crate) enabled_when: Option<&'static str>,
    /// The condition under which the entry shows a **check mark**, or `None` for
    /// a plain (uncheckable) entry. A radio group is several entries whose
    /// `checked_when` keys are mutually exclusive.
    pub(crate) checked_when: Option<&'static str>,
    /// The condition under which the entry is **shown at all**, or `None` for
    /// always — the reference's `on_visible`, unlike `enabled_when` which greys
    /// the line in place.
    pub(crate) visible_when: Option<&'static str>,
    /// The accelerator drawn against the entry (e.g. `"Ctrl+I"`), or `None`.
    /// Display-only here — binding the key globally is the live wiring's job.
    pub(crate) accelerator: Option<&'static str>,
}

impl MenuCommand {
    /// A plain always-available action: a label and the action it emits.
    pub(crate) const fn new(label: &'static str, action: &'static str) -> Self {
        Self {
            label,
            action,
            enabled_when: None,
            checked_when: None,
            visible_when: None,
            accelerator: None,
        }
    }

    /// The same entry with an accelerator label drawn against it.
    #[must_use]
    pub(crate) const fn accel(mut self, accelerator: &'static str) -> Self {
        self.accelerator = Some(accelerator);
        self
    }

    /// The same entry as a check item, checked while `condition` holds.
    #[must_use]
    pub(crate) const fn checked_when(mut self, condition: &'static str) -> Self {
        self.checked_when = Some(condition);
        self
    }

    /// The same entry, enabled only while `condition` holds.
    #[must_use]
    pub(crate) const fn enabled_when(mut self, condition: &'static str) -> Self {
        self.enabled_when = Some(condition);
        self
    }

    /// The same entry, shown only while `condition` holds.
    #[must_use]
    pub(crate) const fn visible_when(mut self, condition: &'static str) -> Self {
        self.visible_when = Some(condition);
        self
    }
}

/// One line in a menu: a command, a submenu, or a rule between groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    variant_size_differences,
    reason = "a `Command` carries its whole declaration inline (label, action, \
              three condition keys, an accelerator) while a `Submenu` is one \
              reference and a `Separator` is empty; the entries live in `static` \
              arrays authored by hand, where by-value commands read far better \
              than a forest of separate `static MenuCommand`s referenced by \
              pointer, and a menu is never large enough for the width to matter"
)]
pub(crate) enum MenuItemDef {
    /// A single command. Greyed if its `enabled_when` fails, absent if its
    /// `visible_when` fails.
    Command(MenuCommand),
    /// A named submenu, opened toward the inline end of its line. Recursive: a
    /// submenu is an ordinary [`MenuDef`].
    Submenu(&'static MenuDef),
    /// A horizontal rule between two groups of entries.
    Separator,
}

/// A menu: the label it drops from, and its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MenuDef {
    /// The button / submenu label this menu drops from.
    pub(crate) label: &'static str,
    /// The lines, in presentation order (top to bottom *is* the layout).
    pub(crate) items: &'static [MenuItemDef],
}

/// A menu bar: an ordered strip of top-level menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MenuBarDef {
    /// The top-level menus, left-to-right in text order.
    pub(crate) menus: &'static [&'static MenuDef],
}

// ---------------------------------------------------------------------------
// Conditions — the same "named key, filled from the world" model as the pie.
// ---------------------------------------------------------------------------

/// The condition key that never holds — the convention for an entry that is
/// present for structure but can never be activated (the top bar's "no entries
/// yet" placeholder). The bar never sets it, so an `enabled_when(NEVER_CONDITION)`
/// entry is always greyed; menu search ([`crate::menu_search`]) skips it, since a
/// permanently unavailable entry is not a real search target.
pub(crate) const NEVER_CONDITION: &str = "never";

/// The conditions that currently hold, by name.
///
/// A component rather than a resource, so two open menus (or a test's fixture
/// and the live bar) do not share one truth. The live viewer fills it from the
/// session ([`crate::menu_bar`]); the gallery and tests leave it empty, and
/// every conditional entry then reads as unavailable / unchecked — a *true*
/// rendering of "no session", not a stub.
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct MenuConditions(pub(crate) Vec<&'static str>);

impl MenuConditions {
    /// Whether a named condition holds. A `None` key always holds; a `Some` key
    /// holds iff it is present.
    pub(crate) fn holds(&self, key: Option<&'static str>) -> bool {
        match key {
            None => true,
            Some(name) => self.0.contains(&name),
        }
    }
}

// ---------------------------------------------------------------------------
// The menu-search filter — the reference's `hightlightAndHide`, applied while a
// popup is built. Set by `crate::menu_search`; read here when a menu opens.
// ---------------------------------------------------------------------------

/// The active menu-search filter.
///
/// While `query` is non-empty, a popup for a menu under `element` is built to
/// show only the entries whose label matches the query (drawn highlighted) — or
/// every entry, under a menu whose own label matched — hiding the rest, the way
/// the reference viewer's `LLStatusBar` filter does (`hightlightAndHide`). An
/// empty `query`, or any menu under a different `element` (the inventory gear, a
/// context menu), builds in full, unfiltered. Set from the search field in
/// [`crate::menu_search`]; a default (empty) filter changes nothing.
#[derive(Resource, Default)]
pub(crate) struct MenuFilter {
    /// The `element` whose menus this filters — [`crate::menu_bar`]'s top bar.
    pub(crate) element: &'static str,
    /// The lower-cased search term; empty means no active filter.
    pub(crate) query: String,
}

impl MenuFilter {
    /// The filter context for building a **top-level** popup of `def` under
    /// `element`, or `None` when no filter applies to it. A top menu whose own
    /// label matches the query shows its whole subtree (`parent_matched`).
    fn context_for(&self, element: &'static str, def: &MenuDef) -> Option<MenuFilterCtx<'_>> {
        if self.query.is_empty() || self.element != element {
            return None;
        }
        Some(MenuFilterCtx {
            query: &self.query,
            parent_matched: label_matches_filter(def.label, &self.query),
        })
    }

    /// The filter context for a **submenu** popup, whose branch recorded whether
    /// an ancestor (or its own label) already matched (`parent_matched`).
    fn context_for_branch(
        &self,
        element: &'static str,
        parent_matched: bool,
    ) -> Option<MenuFilterCtx<'_>> {
        if self.query.is_empty() || self.element != element {
            return None;
        }
        Some(MenuFilterCtx {
            query: &self.query,
            parent_matched,
        })
    }
}

/// The filter in force while one popup is built: the (non-empty, lower-cased)
/// query, and whether an ancestor menu's label already matched it — in which
/// case this whole level is shown, matching the reference's downward
/// `hide = !bHighlighted` propagation.
#[derive(Clone, Copy)]
struct MenuFilterCtx<'a> {
    /// The lower-cased search term.
    query: &'a str,
    /// Whether an ancestor menu (or this menu's own label) matched, so every
    /// entry at this level is shown regardless of its own match.
    parent_matched: bool,
}

/// Whether `label` contains `query` (a lower-cased, non-empty term),
/// case-insensitively — the reference's substring test.
fn label_matches_filter(label: &str, query: &str) -> bool {
    label.to_lowercase().contains(query)
}

/// Whether `def`'s subtree carries a match for `query`: one of its commands'
/// labels, or a submenu label or something inside a submenu. A never-enabled
/// placeholder is not counted, so an unpopulated menu does not read as a hit.
fn subtree_matches_filter(def: &MenuDef, query: &str) -> bool {
    def.items.iter().any(|item| match item {
        MenuItemDef::Command(command) => {
            command.enabled_when != Some(NEVER_CONDITION)
                && label_matches_filter(command.label, query)
        }
        MenuItemDef::Submenu(sub) => {
            label_matches_filter(sub.label, query) || subtree_matches_filter(sub, query)
        }
        MenuItemDef::Separator => false,
    })
}

// ---------------------------------------------------------------------------
// Look and feel. Const paint (so the skinless test / gallery reads right) plus a
// `.sk-menu*` class for a loaded skin's colour / radius; the highlight itself is
// painted by `highlight_menu_hover` so it works with or without a skin.
// ---------------------------------------------------------------------------

/// A menu bar / drop-down surface background.
const MENU_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A menu-bar button / menu entry's resting background (transparent).
const ENTRY_BACKGROUND: Color = Color::NONE;

/// A hovered menu button / entry's background — the highlight.
const ENTRY_HIGHLIGHT: Color = Color::srgb(0.24, 0.34, 0.52);

/// The label colour of an entry that **matched** the active menu-search filter —
/// a warm accent, the reference viewer's `hightlightAndHide` highlight. A
/// build-time text colour, not a per-frame background, so it does not fight the
/// hover highlight ([`highlight_menu_hover`], which paints backgrounds).
const FILTER_MATCH_COLOR: Color = Color::srgb(0.98, 0.82, 0.40);

/// A drop-down's border.
const MENU_BORDER: Color = Color::srgb(0.30, 0.36, 0.46);

/// An enabled entry's label colour.
const ENTRY_TEXT: Color = Color::srgb(0.92, 0.94, 0.98);

/// A disabled entry's label colour — clearly greyed.
const ENTRY_TEXT_DISABLED: Color = Color::srgb(0.45, 0.49, 0.56);

/// An accelerator / submenu-arrow colour — muted against the label.
const ENTRY_ACCESSORY: Color = Color::srgb(0.62, 0.66, 0.74);

/// A separator rule colour.
const SEPARATOR_COLOR: Color = Color::srgb(0.30, 0.34, 0.42);

/// The inline / block padding around a menu-bar button's label, in logical px.
const BAR_BUTTON_PADDING: Vec2 = Vec2::new(12.0, 6.0);

/// The inline / block padding around a drop-down entry's row, in logical px.
const ENTRY_PADDING: Vec2 = Vec2::new(10.0, 5.0);

/// The width of the leading **check gutter** every entry reserves, in logical
/// px, so labels line up whether or not an entry is checked (the reference's
/// `LEFT_WIDTH`). Fixed because it holds a glyph, not text.
const CHECK_GUTTER_WIDTH: f32 = 16.0;

/// The minimum gap between an entry's label and its trailing accessory, so a
/// long label pushes the accessory out rather than overlapping it.
const ACCESSORY_GAP: f32 = 24.0;

/// A drop-down's inner padding, in logical pixels.
const MENU_PADDING: f32 = 4.0;

/// A drop-down's least width, in logical pixels.
const MENU_MIN_WIDTH: f32 = 140.0;

/// The font size a drop-down entry / bar button sets its text at, in logical px.
const ENTRY_FONT: f32 = 15.0;

/// The check-mark glyph. The reference uses U+2714 HEAVY CHECK MARK; we use the
/// lighter U+2713 CHECK MARK, drawn a couple of points smaller than the label
/// ([`CHECK_FONT`]), which reads as a mark against the entry rather than a
/// competing glyph.
const CHECK_GLYPH: &str = "\u{2713}";

/// The font size the check mark is drawn at, in logical pixels — smaller than
/// the label so the mark sits quietly in its gutter.
const CHECK_FONT: f32 = ENTRY_FONT - 3.0;

/// The gap between the check gutter and the entry's label, in logical pixels —
/// logical, so it stays on the label side of the gutter under RTL.
const GUTTER_LABEL_GAP: f32 = 6.0;

/// The submenu-arrow glyph (U+25B6), the reference's `BRANCH_SUFFIX`. One fixed
/// glyph, not mirrored: it means "there is more, toward the inline end", and the
/// popup it points at is placed there too, so under RTL both move together.
const SUBMENU_ARROW: &str = "\u{25B6}";

/// The z-index a menu popup renders at — above every floater and panel.
const MENU_Z_INDEX: i32 = 10_000;

// ---------------------------------------------------------------------------
// Components tying the widget together.
// ---------------------------------------------------------------------------

/// The host of one menu-bar button (or gear button): the button plus, while
/// open, its drop-down. Owns the def to (re)build and the open popup, if any.
#[derive(Component)]
pub(crate) struct MenuHost {
    /// The menu this host drops down.
    def: &'static MenuDef,
    /// The `element` its actions are attributed to.
    element: &'static str,
    /// The open drop-down popup entity, or `None` while closed.
    open: Option<Entity>,
}

/// A menu-bar (or gear) button, so [`highlight_menu_hover`] lights it on hover.
#[derive(Component)]
struct MenuBarButton;

/// A drop-down command line that emits an action when activated. Read by
/// [`emit_menu_action`].
#[derive(Component, Debug, Clone, Copy)]
struct MenuEntryAction {
    /// The `element` the action is attributed to.
    element: &'static str,
    /// The action string emitted.
    action: &'static str,
}

/// A submenu line, marking the [`MenuDef`] it fronts and holding its open child
/// list, so [`manage_submenus`] can open and close it on hover.
#[derive(Component, Debug, Clone, Copy)]
struct MenuBranch {
    /// The submenu this line opens.
    def: &'static MenuDef,
    /// The `element` its entries' actions are attributed to.
    element: &'static str,
    /// The open child-list popup entity, or `None` while closed.
    open: Option<Entity>,
    /// Whether, when this branch was built under a menu-search filter, an
    /// ancestor (or the submenu's own label) already matched — so the branch's
    /// child popup shows its whole level. Meaningless (and `false`) when no
    /// filter was active; read by [`manage_submenus`] to build the child popup.
    filter_parent_matched: bool,
}

/// A free (anchorless) context menu's cursor anchor — the despawn handle for the
/// whole menu, closed by a pick, an outside press or `Escape`.
#[derive(Component)]
struct FreeContextMenu;

// ---------------------------------------------------------------------------
// The menu bar and its buttons.
// ---------------------------------------------------------------------------

/// Spawn a horizontal menu bar under `parent`, one drop-down button per
/// top-level menu, and return its row entity.
///
/// The bar sizes to its buttons and wraps rather than clipping (convention 2),
/// so a larger UI font or a longer translation grows and reflows it.
pub(crate) fn spawn_menu_bar(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    def: &'static MenuBarDef,
    element: &'static str,
) -> Entity {
    let bar = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(2.0), Val::Px(0.0)),
                column_gap: Val::Px(2.0),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(MENU_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-bar"]),
            Name::new("menu-bar"),
            ChildOf(parent),
        ))
        .id();
    for menu in def.menus {
        spawn_menu_button(commands, bar, cx, menu, element);
    }
    bar
}

/// Spawn one menu button under `parent` — a labelled button that drops `def`
/// beneath itself when pressed — and return its host entity.
///
/// The reusable unit shared by the top menu bar and the inventory window's gear
/// / view buttons. Open / close is driven by the press observer on the button
/// ([`toggle_host`]); the `Button` component is kept for keyboard focus, not its
/// activation path.
pub(crate) fn spawn_menu_button(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    def: &'static MenuDef,
    element: &'static str,
) -> Entity {
    let host = commands
        .spawn((
            Node::default(),
            MenuHost {
                def,
                element,
                open: None,
            },
            Name::new(format!("menu-host:{}", def.label)),
            ChildOf(parent),
        ))
        .id();
    commands
        .spawn((
            Button,
            MenuBarButton,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(BAR_BUTTON_PADDING.x), Val::Px(BAR_BUTTON_PADDING.y)),
                ..default()
            },
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-bar-item"]),
            Name::new(format!("menu-button:{}", def.label)),
            ChildOf(host),
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  mut hosts: Query<(Entity, &mut MenuHost)>,
                  conditions: Query<&MenuConditions>,
                  child_of: Query<&ChildOf>,
                  direction: Res<UiDirection>,
                  filter: Res<MenuFilter>,
                  mut commands: Commands| {
                // Consume the press so it does not reach the root dismiss
                // observer (which would close the menu we are about to open).
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                toggle_host(
                    host,
                    &mut hosts,
                    &conditions,
                    &child_of,
                    *direction,
                    &filter,
                    &mut commands,
                );
            },
        )
        .with_child((
            Text::new(cx.text(def.label)),
            cx.font(UiFont::Sans),
            TextColor(ENTRY_TEXT),
            // A child node blocks picking by default, so an un-ignored label
            // would swallow the press and the button would never see it.
            Pickable::IGNORE,
        ));
    host
}

/// Toggle `host`'s drop-down: close the whole bar, then (re)open this one unless
/// it was already the open menu.
///
/// Closing the bar first is what makes clicking straight from one top menu to
/// the next read as *switching* rather than stacking, and matches the reference
/// (at most one bar menu is ever down).
fn toggle_host(
    host: Entity,
    hosts: &mut Query<(Entity, &mut MenuHost)>,
    conditions: &Query<&MenuConditions>,
    child_of: &Query<&ChildOf>,
    direction: UiDirection,
    filter: &MenuFilter,
    commands: &mut Commands,
) {
    let was_open = hosts.get(host).is_ok_and(|(_, menu)| menu.open.is_some());
    close_all_hosts(hosts, commands);
    if !was_open {
        let held = conditions_at(host, child_of, conditions);
        if let Ok((_, mut menu)) = hosts.get_mut(host) {
            open_host(&mut menu, host, held, direction, filter, commands);
        }
    }
}

/// Once one bar menu is open, hovering a different top-level button opens *that*
/// one — the reference's `LLMenuBarGL::handleHover`, so the bar reads like one
/// strip you sweep across rather than a row you must click each of.
///
/// Gated on a menu already being open: the *first* menu still opens on a click
/// (a bare hover over the bar does nothing), matching the reference.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the hover map, the \
              ancestry and bar-button queries, the live conditions, the layout direction, the \
              menu-search filter, the hosts to (re)open and commands to do it with"
)]
fn switch_menu_on_hover(
    hover: Res<HoverMap>,
    child_of: Query<&ChildOf>,
    buttons: Query<&ChildOf, With<MenuBarButton>>,
    conditions: Query<&MenuConditions>,
    direction: Res<UiDirection>,
    filter: Res<MenuFilter>,
    mut hosts: Query<(Entity, &mut MenuHost)>,
    mut commands: Commands,
) {
    if !hosts.iter().any(|(_, menu)| menu.open.is_some()) {
        return;
    }
    let mut hovered = HashSet::new();
    for hits in hover.values() {
        for hit in hits.keys() {
            hovered.insert(*hit);
            for ancestor in child_of.iter_ancestors(*hit) {
                hovered.insert(ancestor);
            }
        }
    }
    let Some(host) = hovered
        .iter()
        .find_map(|entity| buttons.get(*entity).ok().map(ChildOf::parent))
    else {
        return;
    };
    // Only switch *to* a closed menu; hovering the already-open one is a no-op
    // (toggling it would close the menu the pointer is on).
    if hosts.get(host).is_ok_and(|(_, menu)| menu.open.is_none()) {
        toggle_host(
            host,
            &mut hosts,
            &conditions,
            &child_of,
            *direction,
            &filter,
            &mut commands,
        );
    }
}

/// Close every open bar menu.
fn close_all_hosts(hosts: &mut Query<(Entity, &mut MenuHost)>, commands: &mut Commands) {
    for (_, mut menu) in hosts.iter_mut() {
        close_host(&mut menu, commands);
    }
}

/// Open the first bar menu that carries a match whenever the menu-search filter
/// changes, so typing a term *shows* its result rather than waiting for the user
/// to open a menu by hand.
///
/// "First" is bar order — the child order of the bar row — so the leftmost menu
/// with at least one matching entry opens; the rest stay closed. Each filter
/// change closes every menu under the filtered element and reopens the target
/// against the current term, so refining the term rebuilds the open drop-down;
/// clearing the term closes it. Runs only on a real filter change
/// ([`MenuFilter`]'s change detection), so a menu opened or closed by hand while
/// the term is steady is left alone.
fn open_filtered_menu(
    filter: Res<MenuFilter>,
    conditions: Query<&MenuConditions>,
    child_of: Query<&ChildOf>,
    children: Query<&Children>,
    direction: Res<UiDirection>,
    mut hosts: Query<(Entity, &mut MenuHost)>,
    mut commands: Commands,
) {
    if !filter.is_changed() {
        return;
    }
    // The bar row holding the filtered element's hosts, walked in child order.
    let bar = hosts
        .iter()
        .find(|(_, menu)| menu.element == filter.element)
        .and_then(|(host, _)| child_of.get(host).ok())
        .map(ChildOf::parent);
    // The target: the first host, in bar order, whose subtree carries a match.
    let target = if filter.query.is_empty() {
        None
    } else {
        bar.and_then(|bar| children.get(bar).ok()).and_then(|kids| {
            kids.iter().find(|&child| {
                hosts.get(child).is_ok_and(|(_, menu)| {
                    menu.element == filter.element
                        && subtree_matches_filter(menu.def, &filter.query)
                })
            })
        })
    };
    // Close every host under the element, then (re)open the target so its popup
    // reflects the current term.
    for (host_entity, mut menu) in hosts.iter_mut() {
        if menu.element != filter.element {
            continue;
        }
        close_host(&mut menu, &mut commands);
        if Some(host_entity) == target {
            let held = conditions_at(host_entity, &child_of, &conditions);
            open_host(
                &mut menu,
                host_entity,
                held,
                *direction,
                &filter,
                &mut commands,
            );
        }
    }
}

/// Build and attach `host`'s drop-down.
fn open_host(
    host_menu: &mut MenuHost,
    host: Entity,
    conditions: Option<&MenuConditions>,
    direction: UiDirection,
    filter: &MenuFilter,
    commands: &mut Commands,
) {
    let empty = MenuConditions::default();
    let held = conditions.unwrap_or(&empty);
    let popup = build_menu_popup(
        commands,
        host,
        host_menu.def,
        host_menu.element,
        held,
        DropDirection::Block,
        direction,
        filter.context_for(host_menu.element, host_menu.def),
    );
    host_menu.open = Some(popup);
}

/// Despawn `host`'s drop-down (and any submenus under it), if open.
fn close_host(host_menu: &mut MenuHost, commands: &mut Commands) {
    if let Some(popup) = host_menu.open.take() {
        commands.entity(popup).despawn();
    }
}

/// Which way a popup drops relative to its anchor.
#[derive(Clone, Copy)]
enum DropDirection {
    /// Down, aligned to the anchor's inline start — a top-level or gear menu.
    Block,
    /// Toward the inline end, aligned to the anchor's block start — a submenu.
    Inline,
}

impl DropDirection {
    /// The [`Popover`] candidate placements for this direction, most-preferred
    /// first, each with an edge fallback — built from the logical drop folded
    /// against `direction`, so the whole thing mirrors under RTL.
    fn placements(self, direction: UiDirection) -> Vec<PopoverPlacement> {
        let (inline_start_side, inline_end_side) = match direction {
            UiDirection::Ltr => (PopoverSide::Left, PopoverSide::Right),
            UiDirection::Rtl => (PopoverSide::Right, PopoverSide::Left),
        };
        // For a below-drop, `Start`/`End` are the inline extremes measured
        // left-to-right, so inline-start is `Start` under LTR, `End` under RTL.
        let inline_start_align = match direction {
            UiDirection::Ltr => PopoverAlign::Start,
            UiDirection::Rtl => PopoverAlign::End,
        };
        match self {
            Self::Block => vec![
                PopoverPlacement {
                    side: PopoverSide::Bottom,
                    align: inline_start_align,
                    gap: 0.0,
                },
                PopoverPlacement {
                    side: PopoverSide::Top,
                    align: inline_start_align,
                    gap: 0.0,
                },
            ],
            Self::Inline => vec![
                PopoverPlacement {
                    side: inline_end_side,
                    align: PopoverAlign::Start,
                    gap: 0.0,
                },
                PopoverPlacement {
                    side: inline_start_side,
                    align: PopoverAlign::Start,
                    gap: 0.0,
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// The drop-down list itself.
// ---------------------------------------------------------------------------

/// Build a drop-down popup for `def` under `anchor`, and return it.
///
/// A column of entry rows positioned against `anchor` by [`Popover`], built
/// fresh on each open so its check / enabled / visible states reflect the
/// conditions that hold *now*.
#[expect(
    clippy::too_many_arguments,
    reason = "the popup builder takes each of the independent inputs its caller supplies: the \
              spawn target, the menu to build, the element its picks are attributed to, the live \
              conditions, the drop and layout directions, and the optional menu-search filter"
)]
fn build_menu_popup(
    commands: &mut Commands,
    anchor: Entity,
    def: &'static MenuDef,
    element: &'static str,
    conditions: &MenuConditions,
    drop: DropDirection,
    direction: UiDirection,
    filter: Option<MenuFilterCtx>,
) -> Entity {
    let popup = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(MENU_PADDING)),
                border: UiRect::all(Val::Px(1.0)),
                min_width: Val::Px(MENU_MIN_WIDTH),
                // Align children to the start, not the default stretch. An
                // absolutely-positioned flex column that *stretches* its children
                // on the cross axis is grown on the **main** (block) axis too by
                // taffy — the popup ends up far taller than its rows, leaving dead
                // space below the last entry (starkly visible on a one-line menu
                // like the "(no entries yet)" placeholder). Rows and separators
                // fill the width by an explicit `width: 100%` instead, which does
                // not trip the quirk.
                align_items: AlignItems::Start,
                ..column(Val::Px(0.0))
            },
            Popover {
                positions: drop.placements(direction),
                window_margin: 4.0,
            },
            BackgroundColor(MENU_BACKGROUND),
            BorderColor::all(MENU_BORDER),
            GlobalZIndex(MENU_Z_INDEX),
            ClassList::new_with_classes(["sk-menu"]),
            Name::new(format!("menu-popup:{}", def.label)),
            ChildOf(anchor),
        ))
        // Consume a press that lands on the popup's own padding / border, so it
        // does not bubble to the root dismiss observer and close the menu.
        .observe(|mut press: On<Pointer<Press>>| press.propagate(false))
        .id();
    for item in def.items {
        spawn_menu_line(commands, popup, *item, element, conditions, filter);
    }
    popup
}

/// Spawn one drop-down line — a command, a submenu, or a separator.
///
/// With `filter` set (a menu-search term in force), the reference's
/// `hightlightAndHide` applies: a command is shown only if its label matches (or
/// an ancestor menu already matched), drawn highlighted on its own match; a
/// submenu is shown only if its subtree carries a match; and separators are
/// dropped, since the groups they divide are being filtered anyway.
fn spawn_menu_line(
    commands: &mut Commands,
    popup: Entity,
    item: MenuItemDef,
    element: &'static str,
    conditions: &MenuConditions,
    filter: Option<MenuFilterCtx>,
) {
    match item {
        MenuItemDef::Command(command) => {
            if !conditions.holds(command.visible_when) {
                return;
            }
            match filter {
                None => spawn_command_line(commands, popup, command, element, conditions, false),
                Some(ctx) => {
                    let own_match = label_matches_filter(command.label, ctx.query);
                    if ctx.parent_matched || own_match {
                        spawn_command_line(
                            commands, popup, command, element, conditions, own_match,
                        );
                    }
                }
            }
        }
        MenuItemDef::Submenu(sub) => match filter {
            None => spawn_submenu_line(commands, popup, sub, element, false, false),
            Some(ctx) => {
                let own_match = label_matches_filter(sub.label, ctx.query);
                let child_parent_matched = ctx.parent_matched || own_match;
                if child_parent_matched || subtree_matches_filter(sub, ctx.query) {
                    spawn_submenu_line(
                        commands,
                        popup,
                        sub,
                        element,
                        child_parent_matched,
                        own_match,
                    );
                }
            }
        },
        MenuItemDef::Separator => {
            if filter.is_none() {
                spawn_separator_line(commands, popup);
            }
        }
    }
}

/// Spawn a command line: [check gutter] [label] [accelerator].
///
/// `highlight` draws the label in the menu-search accent ([`FILTER_MATCH_COLOR`])
/// — set when the entry itself matched an active filter; a disabled entry stays
/// greyed regardless.
fn spawn_command_line(
    commands: &mut Commands,
    popup: Entity,
    command: MenuCommand,
    element: &'static str,
    conditions: &MenuConditions,
    highlight: bool,
) {
    let enabled = conditions.holds(command.enabled_when);
    let checked = command.checked_when.is_some() && conditions.holds(command.checked_when);
    let text_color = if !enabled {
        ENTRY_TEXT_DISABLED
    } else if highlight {
        FILTER_MATCH_COLOR
    } else {
        ENTRY_TEXT
    };
    let action = command.action;
    let row = commands
        .spawn((
            entry_row_node(),
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-item"]),
            MenuEntryAction { element, action },
            Name::new(format!("menu-item:{}", command.action)),
            ChildOf(popup),
        ))
        .id();
    if !enabled {
        commands.entity(row).insert(InteractionDisabled);
    }
    if checked {
        commands.entity(row).insert(Checked);
    }
    // Emission is a single point — an `Activate` observer — so a press (mouse)
    // and the harness (`activate`) both dispatch the one way. The press also
    // closes the stack.
    commands.entity(row).observe(emit_menu_action).observe(
        move |mut press: On<Pointer<Press>>,
              disabled: Query<Has<InteractionDisabled>>,
              mut hosts: Query<(Entity, &mut MenuHost)>,
              free: Query<Entity, With<FreeContextMenu>>,
              mut commands: Commands| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            if disabled.get(row).unwrap_or(false) {
                return;
            }
            commands.trigger(Activate { entity: row });
            dismiss_all(&mut hosts, &free, &mut commands);
        },
    );
    spawn_gutter(
        commands,
        row,
        if checked { CHECK_GLYPH } else { "" },
        text_color,
    );
    spawn_entry_label(commands, row, command.label, text_color);
    if let Some(accelerator) = command.accelerator {
        commands.spawn((
            Text::new(accelerator),
            UiFont::Sans.at(ENTRY_FONT),
            TextColor(ENTRY_ACCESSORY),
            Pickable::IGNORE,
            Name::new("menu-item-accel"),
            ChildOf(row),
        ));
    }
}

/// Spawn a submenu line: [gutter] [label] [arrow]. The child list opens lazily
/// on hover ([`manage_submenus`]); its own press is only consumed, so clicking a
/// branch does not dismiss the menu.
fn spawn_submenu_line(
    commands: &mut Commands,
    popup: Entity,
    sub: &'static MenuDef,
    element: &'static str,
    filter_parent_matched: bool,
    highlight: bool,
) {
    let label_color = if highlight {
        FILTER_MATCH_COLOR
    } else {
        ENTRY_TEXT
    };
    let row = commands
        .spawn((
            entry_row_node(),
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-item"]),
            MenuBranch {
                def: sub,
                element,
                open: None,
                filter_parent_matched,
            },
            Name::new(format!("menu-submenu:{}", sub.label)),
            ChildOf(popup),
        ))
        .observe(|mut press: On<Pointer<Press>>| press.propagate(false))
        .id();
    spawn_gutter(commands, row, "", label_color);
    spawn_entry_label(commands, row, sub.label, label_color);
    commands.spawn((
        Text::new(SUBMENU_ARROW),
        UiFont::Sans.at(ENTRY_FONT),
        TextColor(ENTRY_ACCESSORY),
        Pickable::IGNORE,
        Name::new("menu-submenu-arrow"),
        ChildOf(row),
    ));
}

/// The shared row node of a command / submenu line.
fn entry_row_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        // Fill the popup width by a percentage, not a cross-axis stretch — the
        // popup aligns its children to the start to avoid a taffy height quirk
        // (see `build_menu_popup`), so every row asks for the full width itself.
        width: Val::Percent(100.0),
        padding: UiRect::axes(Val::Px(ENTRY_PADDING.x), Val::Px(ENTRY_PADDING.y)),
        column_gap: Val::Px(4.0),
        ..default()
    }
}

/// Spawn an entry's leading check gutter, holding `glyph` (empty for none).
///
/// `Pickable::IGNORE`, like every entry child, so the pointer's target is the
/// **row**, not this child.
fn spawn_gutter(commands: &mut Commands, row: Entity, glyph: &str, color: Color) {
    commands.spawn((
        Node {
            width: Val::Px(CHECK_GUTTER_WIDTH),
            flex_shrink: 0.0,
            ..default()
        },
        // A logical gap on the label side of the gutter, so the check sits a
        // little clear of the text (and stays clear of it under RTL).
        LogicalMargin(LogicalRect {
            inline_end: Val::Px(GUTTER_LABEL_GAP),
            ..LogicalRect::ZERO
        }),
        Text::new(glyph),
        UiFont::Sans.at(CHECK_FONT),
        TextColor(color),
        Pickable::IGNORE,
        Name::new("menu-item-check"),
        ChildOf(row),
    ));
}

/// Spawn an entry's growing label, reserving a trailing gap for its accessory.
fn spawn_entry_label(commands: &mut Commands, row: Entity, label: &str, color: Color) {
    commands.spawn((
        Node {
            flex_grow: 1.0,
            margin: UiRect::right(Val::Px(ACCESSORY_GAP)),
            ..default()
        },
        Text::new(label),
        UiFont::Sans.at(ENTRY_FONT),
        TextColor(color),
        Pickable::IGNORE,
        Name::new("menu-item-label"),
        ChildOf(row),
    ));
}

/// Spawn a separator line — one faint rule, not pickable.
fn spawn_separator_line(commands: &mut Commands, popup: Entity) {
    commands.spawn((
        Node {
            height: Val::Px(1.0),
            // Fill the popup width via a percentage, not a cross-axis stretch:
            // the popup aligns its children to the start to dodge a taffy quirk
            // (see `build_menu_popup`), so a rule that relied on stretch would
            // collapse to zero width. The horizontal inset comes from the popup's
            // own padding rather than a margin that would overflow the 100%.
            width: Val::Percent(100.0),
            margin: UiRect::axes(Val::Px(0.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(SEPARATOR_COLOR),
        ClassList::new_with_classes(["sk-menu-separator"]),
        Pickable::IGNORE,
        Name::new("menu-separator"),
        ChildOf(popup),
    ));
}

/// Observer on a command entry: write its [`UiAction`] when activated. The whole
/// of an entry's outward wiring — the viewer routes it, the gallery drops it, a
/// test reads it (the registry rule, [`crate::ui_element`]).
fn emit_menu_action(
    activate: On<Activate>,
    entries: Query<&MenuEntryAction>,
    mut actions: MessageWriter<UiAction>,
) {
    if let Ok(entry) = entries.get(activate.entity) {
        actions.write(UiAction {
            element: entry.element,
            action: entry.action,
        });
    }
}

// ---------------------------------------------------------------------------
// Submenus — hover-driven.
// ---------------------------------------------------------------------------

/// Keep each submenu open exactly while its branch is under the pointer.
///
/// "Under the pointer" means the branch **row or anything in its subtree** — and
/// because a branch's open child list is spawned as a *child of the branch row*,
/// the child list is part of that subtree. So the pointer moving from a branch
/// into its submenu keeps the chain open; moving to a sibling drops it.
fn manage_submenus(
    hover: Res<HoverMap>,
    child_of: Query<&ChildOf>,
    conditions: Query<&MenuConditions>,
    direction: Res<UiDirection>,
    filter: Res<MenuFilter>,
    mut branches: Query<(Entity, &mut MenuBranch)>,
    mut commands: Commands,
) {
    let mut hovered = HashSet::new();
    for hits in hover.values() {
        for hit in hits.keys() {
            hovered.insert(*hit);
            for ancestor in child_of.iter_ancestors(*hit) {
                hovered.insert(ancestor);
            }
        }
    }
    for (branch_entity, mut branch) in &mut branches {
        let active = hovered.contains(&branch_entity);
        match (active, branch.open) {
            (true, None) => {
                let held = conditions_at(branch_entity, &child_of, &conditions);
                let empty = MenuConditions::default();
                let popup = build_menu_popup(
                    &mut commands,
                    branch_entity,
                    branch.def,
                    branch.element,
                    held.unwrap_or(&empty),
                    DropDirection::Inline,
                    *direction,
                    filter.context_for_branch(branch.element, branch.filter_parent_matched),
                );
                branch.open = Some(popup);
            }
            (false, Some(popup)) => {
                commands.entity(popup).despawn();
                branch.open = None;
            }
            (true, Some(_)) | (false, None) => {}
        }
    }
}

/// The [`MenuConditions`] on `entity` or the nearest ancestor that carries them.
///
/// The top menu bar puts one [`MenuConditions`] on its bar row and every button
/// under it inherits it by ancestry, while a gear button that wants its own
/// carries them directly (self wins over an ancestor).
fn conditions_at<'q>(
    entity: Entity,
    child_of: &Query<&ChildOf>,
    conditions: &'q Query<&MenuConditions>,
) -> Option<&'q MenuConditions> {
    conditions.get(entity).ok().or_else(|| {
        child_of
            .iter_ancestors(entity)
            .find_map(|ancestor| conditions.get(ancestor).ok())
    })
}

// ---------------------------------------------------------------------------
// The free (anchorless) context menu — a menu at a screen point.
// ---------------------------------------------------------------------------

/// Open a [`MenuDef`] at a screen point, with no anchor button — the shape a
/// right-click context menu uses.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenContextMenu {
    /// The menu to show.
    pub(crate) menu: &'static MenuDef,
    /// Where to place its corner, in logical pixels.
    pub(crate) at: Vec2,
    /// The `element` its actions are attributed to.
    pub(crate) element: &'static str,
}

/// Spawn a popup for each [`OpenContextMenu`] request, anchored to a zero-size
/// node at the cursor so [`Popover`] positions it against a point. Any previous
/// free menu is cleared first, so a second right-click moves the menu.
fn open_context_menus(
    mut requests: MessageReader<OpenContextMenu>,
    root: Res<UiRoot>,
    direction: Res<UiDirection>,
    existing: Query<Entity, With<FreeContextMenu>>,
    mut commands: Commands,
) {
    for request in requests.read() {
        for anchor in &existing {
            commands.entity(anchor).despawn();
        }
        let anchor = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(request.at.x),
                    top: Val::Px(request.at.y),
                    ..default()
                },
                FreeContextMenu,
                Name::new("context-menu-anchor"),
                ChildOf(root.0),
            ))
            .id();
        build_menu_popup(
            &mut commands,
            anchor,
            request.menu,
            request.element,
            &MenuConditions::default(),
            DropDirection::Block,
            *direction,
            // A context menu is not the searched element, so it is never filtered.
            None,
        );
    }
}

// ---------------------------------------------------------------------------
// Dismissal — outside press and Escape.
// ---------------------------------------------------------------------------

/// Attach the outside-press dismiss observer to the UI root, once the root
/// exists.
fn attach_menu_dismiss(root: Res<UiRoot>, mut commands: Commands) {
    commands.entity(root.0).observe(dismiss_menus_on_press);
}

/// Dismiss every open menu when a press reaches the UI root.
///
/// A press that lands on a menu button or entry is consumed there
/// (`propagate(false)`), so any press that bubbles all the way up to the root is
/// outside every menu — the reference's click-away dismissal, with no dependence
/// on the hover map.
fn dismiss_menus_on_press(
    _press: On<Pointer<Press>>,
    mut hosts: Query<(Entity, &mut MenuHost)>,
    free: Query<Entity, With<FreeContextMenu>>,
    mut commands: Commands,
) {
    dismiss_all(&mut hosts, &free, &mut commands);
}

/// Dismiss every open menu on `Escape`.
fn dismiss_menus_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    free: Query<Entity, With<FreeContextMenu>>,
    mut hosts: Query<(Entity, &mut MenuHost)>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        dismiss_all(&mut hosts, &free, &mut commands);
    }
}

/// Close every open bar menu and despawn every free context menu.
fn dismiss_all(
    hosts: &mut Query<(Entity, &mut MenuHost)>,
    free: &Query<Entity, With<FreeContextMenu>>,
    commands: &mut Commands,
) {
    close_all_hosts(hosts, commands);
    for anchor in free {
        commands.entity(anchor).despawn();
    }
}

// ---------------------------------------------------------------------------
// Highlight.
// ---------------------------------------------------------------------------

/// Highlight the menu button / entry / submenu row under the pointer, and clear
/// the rest.
///
/// The widget's own highlight, because bevy_flair's `:hover` does not read the
/// same in the gallery and the viewer for these rows — so the reference
/// behaviour of the thing under the cursor lighting up is driven here off the
/// hover map. A disabled entry never lights up. The hovered node is usually a
/// child (a label), so each hovered entity is resolved to its owning row.
#[expect(
    clippy::type_complexity,
    reason = "an ordinary Bevy query: the row entity, its background to repaint, \
              and whether it is disabled, filtered to the three row markers — an \
              alias for the tuple would obscure it, not clarify it"
)]
fn highlight_menu_hover(
    hover: Res<HoverMap>,
    child_of: Query<&ChildOf>,
    mut rows: Query<
        (Entity, &mut BackgroundColor, Has<InteractionDisabled>),
        Or<(With<MenuEntryAction>, With<MenuBranch>, With<MenuBarButton>)>,
    >,
) {
    let row_entities: HashSet<Entity> = rows.iter().map(|(entity, _, _)| entity).collect();
    let mut hovered_rows = HashSet::new();
    for hits in hover.values() {
        for hit in hits.keys() {
            if row_entities.contains(hit) {
                hovered_rows.insert(*hit);
            } else if let Some(row) = child_of
                .iter_ancestors(*hit)
                .find(|ancestor| row_entities.contains(ancestor))
            {
                hovered_rows.insert(row);
            }
        }
    }
    for (entity, mut background, disabled) in &mut rows {
        let wanted = if hovered_rows.contains(&entity) && !disabled {
            ENTRY_HIGHLIGHT
        } else {
            ENTRY_BACKGROUND
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// The line-menu widget's runtime.
pub(crate) struct MenuWidgetPlugin;

impl Plugin for MenuWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenContextMenu>()
            .init_resource::<MenuFilter>()
            .add_systems(
                Startup,
                attach_menu_dismiss.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_context_menus,
                    switch_menu_on_hover,
                    open_filtered_menu,
                    manage_submenus,
                    dismiss_menus_on_escape,
                    highlight_menu_hover,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// The gallery / test fixture — one bar exercising every entry kind.
// ---------------------------------------------------------------------------

/// A submenu under the fixture's "World" menu, so the fixture exercises nesting.
static FIXTURE_SUBMENU: MenuDef = MenuDef {
    label: "Environment",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Sunrise", "env-sunrise")),
        MenuItemDef::Command(MenuCommand::new("Midday", "env-midday")),
        MenuItemDef::Command(MenuCommand::new("Sunset", "env-sunset")),
    ],
};

/// The fixture's "Avatar" menu — a check item, a disabled item, accelerators.
static FIXTURE_AVATAR: MenuDef = MenuDef {
    label: "Avatar",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Inventory", "inventory").accel("Ctrl+I")),
        MenuItemDef::Command(MenuCommand::new("Appearance", "appearance")),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Fly", "fly")
                .checked_when("flying")
                .accel("Home"),
        ),
        MenuItemDef::Command(MenuCommand::new("Sit Down", "sit").enabled_when("can-sit")),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Quit", "quit").accel("Ctrl+Q")),
    ],
};

/// The fixture's "World" menu, holding the submenu.
static FIXTURE_WORLD: MenuDef = MenuDef {
    label: "World",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Mini-Map", "mini-map").accel("Ctrl+Shift+M")),
        MenuItemDef::Submenu(&FIXTURE_SUBMENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Teleport Home", "teleport-home")),
        // Shown only under an "advanced" condition — a demo of `on_visible`,
        // absent in the gallery (no conditions), present in the test that sets it.
        MenuItemDef::Command(
            MenuCommand::new("Region Debug Console", "region-debug").visible_when("advanced"),
        ),
    ],
};

/// The fixture menu bar, referenced by the gallery specimen and the tests.
pub(crate) static FIXTURE_MENU_BAR: MenuBarDef = MenuBarDef {
    menus: &[&FIXTURE_AVATAR, &FIXTURE_WORLD],
};

/// The fixture context menu, opened by the gallery's right-click toggle.
pub(crate) static FIXTURE_CONTEXT_MENU: MenuDef = FIXTURE_AVATAR;

/// Spawn the gallery's menu-bar specimen — the closed bar, whose menus open when
/// clicked (never a pre-opened menu). Registered in
/// [`crate::ui_element::ELEMENTS`].
pub(crate) fn spawn_menu_bar_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_menu_bar(commands, parent, cx, &FIXTURE_MENU_BAR, "menu-bar-specimen")
}

#[cfg(test)]
mod tests {
    use super::{
        CHECK_GLYPH, DropDirection, FIXTURE_AVATAR, FIXTURE_MENU_BAR, FIXTURE_WORLD, MenuBranch,
        MenuConditions, MenuDef, MenuEntryAction, MenuHost, MenuItemDef, SUBMENU_ARROW,
        build_menu_popup, spawn_menu_bar_specimen,
    };
    use bevy::picking::hover::HoverMap;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use crate::ui::{UiDirection, UiRoot, UiScaffoldSystems};
    use crate::ui_element::{ElementCx, UiAction};
    use crate::ui_test::{
        LayoutTest, TestError, activate, drain_actions, enable_action_recording, find_by_name,
        settle,
    };

    /// Every command action in a menu, depth-first, tagged with the `>`-joined
    /// path of menu labels that reaches it — the line-menu analogue of the pie's
    /// address table. Pinned so moving an entry is a deliberate diff.
    fn action_paths(menu: &MenuDef, prefix: &str) -> Vec<(String, &'static str)> {
        let here = if prefix.is_empty() {
            menu.label.to_owned()
        } else {
            format!("{prefix} > {}", menu.label)
        };
        let mut out = Vec::new();
        for item in menu.items {
            match item {
                MenuItemDef::Command(command) => out.push((here.clone(), command.action)),
                MenuItemDef::Submenu(sub) => out.extend(action_paths(sub, &here)),
                MenuItemDef::Separator => {}
            }
        }
        out
    }

    /// The fixture bar's entire action table, pinned against a hand-written list.
    #[test]
    fn the_fixture_action_table_is_pinned() {
        let mut table = Vec::new();
        for menu in FIXTURE_MENU_BAR.menus {
            table.extend(action_paths(menu, ""));
        }
        let expected = vec![
            ("Avatar".to_owned(), "inventory"),
            ("Avatar".to_owned(), "appearance"),
            ("Avatar".to_owned(), "fly"),
            ("Avatar".to_owned(), "sit"),
            ("Avatar".to_owned(), "quit"),
            ("World".to_owned(), "mini-map"),
            ("World > Environment".to_owned(), "env-sunrise"),
            ("World > Environment".to_owned(), "env-midday"),
            ("World > Environment".to_owned(), "env-sunset"),
            ("World".to_owned(), "teleport-home"),
            ("World".to_owned(), "region-debug"),
        ];
        assert_eq!(table, expected);
    }

    /// No two commands in one menu share an action string.
    #[test]
    fn no_menu_repeats_an_action() {
        for menu in FIXTURE_MENU_BAR.menus {
            let actions: Vec<&str> = action_paths(menu, "")
                .into_iter()
                .map(|(_, action)| action)
                .collect();
            let mut unique = actions.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(actions.len(), unique.len(), "a menu repeats an action");
        }
    }

    /// `MenuConditions::holds` — `None` always holds, a named key holds iff set.
    #[test]
    fn conditions_gate_named_keys() {
        let held = MenuConditions(vec!["flying"]);
        assert!(held.holds(None));
        assert!(held.holds(Some("flying")));
        assert!(!held.holds(Some("can-sit")));
    }

    /// Spawn a drop-down for `menu` under a fresh root, with `conditions` held,
    /// and settle its layout.
    fn popup_app(menu: &'static MenuDef, conditions: &[&'static str]) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        enable_action_recording(&mut app);
        let held = MenuConditions(conditions.to_vec());
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<UiRoot>| {
                let anchor = commands.spawn((Node::default(), ChildOf(root.0))).id();
                build_menu_popup(
                    &mut commands,
                    anchor,
                    menu,
                    "test",
                    &held,
                    DropDirection::Block,
                    UiDirection::Ltr,
                    None,
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// Spawn a drop-down for `menu` under a filter `query`, and settle its layout.
    /// The filter's `parent_matched` is seeded from whether `menu`'s own label
    /// matches, exactly as [`open_host`](super::open_host) does for a top menu.
    fn filtered_popup_app(menu: &'static MenuDef, query: &str) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        enable_action_recording(&mut app);
        let query = query.to_lowercase();
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<UiRoot>| {
                let anchor = commands.spawn((Node::default(), ChildOf(root.0))).id();
                let ctx = super::MenuFilterCtx {
                    query: &query,
                    parent_matched: super::label_matches_filter(menu.label, &query),
                };
                build_menu_popup(
                    &mut commands,
                    anchor,
                    menu,
                    "test",
                    &MenuConditions::default(),
                    DropDirection::Block,
                    UiDirection::Ltr,
                    Some(ctx),
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// The specimen spawns **closed** — a host per top menu, no popup — so the
    /// gallery never shows a pre-opened menu.
    #[test]
    fn the_specimen_spawns_closed() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                spawn_menu_bar_specimen(&mut commands, root.0, ElementCx::new());
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        let hosts = app
            .world_mut()
            .query::<&MenuHost>()
            .iter(app.world())
            .count();
        assert_eq!(hosts, 2, "one host per top-level fixture menu");
        assert!(
            find_by_name(&mut app, "menu-popup:Avatar").is_none(),
            "no menu is open on a freshly spawned bar"
        );
        Ok(())
    }

    /// An opened menu lays out every entry kind: the visible commands, the check
    /// on a held item, the greying of a failed enable, the accelerators, and the
    /// separators.
    #[test]
    fn an_opened_menu_lays_out_its_entries() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_AVATAR, &["flying"])?;

        let commands = app
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(app.world())
            .count();
        assert_eq!(
            commands, 5,
            "five commands: two separators are not commands"
        );

        let checks = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .filter(|text| text.0 == CHECK_GLYPH)
            .count();
        assert_eq!(checks, 1, "only the held check item shows a check mark");

        let sit = find_by_name(&mut app, "menu-item:sit").ok_or("the Sit entry did not spawn")?;
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(sit)
                .is_some(),
            "an entry whose enable condition fails is disabled"
        );

        let accelerators: Vec<String> = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        for accelerator in ["Ctrl+I", "Home", "Ctrl+Q"] {
            assert!(
                accelerators.iter().any(|text| text == accelerator),
                "the {accelerator} accelerator is drawn against its entry"
            );
        }

        let separators = count_named(&mut app, "menu-separator");
        assert_eq!(separators, 2, "both separators are drawn");
        Ok(())
    }

    /// `visible_when` removes a line when its condition fails and restores it when
    /// it holds — unlike a failed `enabled_when`, which greys the line in place.
    #[test]
    fn visible_when_adds_and_removes_a_line() -> Result<(), TestError> {
        let mut hidden = popup_app(&FIXTURE_WORLD, &[])?;
        assert!(
            action_entity(&mut hidden, "region-debug").is_none(),
            "the advanced entry is absent without its condition"
        );
        let mut shown = popup_app(&FIXTURE_WORLD, &["advanced"])?;
        assert!(
            action_entity(&mut shown, "region-debug").is_some(),
            "the advanced entry appears when its condition holds"
        );
        Ok(())
    }

    /// A submenu row fronts its child menu with a branch arrow, and names it.
    #[test]
    fn a_submenu_row_fronts_its_child() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_WORLD, &[])?;
        let branches: Vec<&'static str> = app
            .world_mut()
            .query::<&MenuBranch>()
            .iter(app.world())
            .map(|branch| branch.def.label)
            .collect();
        assert_eq!(branches, vec!["Environment"], "one submenu, named");
        let arrows = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .filter(|text| text.0 == SUBMENU_ARROW)
            .count();
        assert_eq!(arrows, 1, "the submenu row draws one branch arrow");
        Ok(())
    }

    /// Activating an entry writes its `UiAction` and nothing else — the whole of
    /// its outward wiring, and the one point a mouse press and a test share.
    #[test]
    fn activating_an_entry_emits_its_action() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_AVATAR, &[])?;
        let quit =
            find_by_name(&mut app, "menu-item:quit").ok_or("the Quit entry did not spawn")?;
        activate(&mut app, quit);
        let actions = drain_actions(&mut app);
        assert_eq!(
            actions,
            vec![UiAction {
                element: "test",
                action: "quit",
            }],
        );
        Ok(())
    }

    /// `subtree_matches_filter` sees into submenus and past the never-enabled
    /// placeholder.
    #[test]
    fn subtree_match_sees_into_submenus() {
        // "sunset" is only inside World's Environment submenu.
        assert!(super::subtree_matches_filter(&FIXTURE_WORLD, "sunset"));
        // "teleport" is a top-level World command.
        assert!(super::subtree_matches_filter(&FIXTURE_WORLD, "teleport"));
        // Nothing in World mentions "inventory".
        assert!(!super::subtree_matches_filter(&FIXTURE_WORLD, "inventory"));
    }

    /// A filter shows only the matching command and hides the rest.
    #[test]
    fn a_filter_hides_non_matching_commands() -> Result<(), TestError> {
        let mut app = filtered_popup_app(&FIXTURE_AVATAR, "fl")?;
        let commands = app
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(app.world())
            .count();
        assert_eq!(commands, 1, "only the matching Fly entry is shown");
        assert!(
            action_entity(&mut app, "fly").is_some(),
            "the matching entry is present",
        );
        assert!(
            action_entity(&mut app, "inventory").is_none(),
            "a non-matching entry is hidden",
        );
        Ok(())
    }

    /// A filter that matches the menu's own label shows the whole menu — the
    /// reference's downward "show everything under a matched menu" propagation.
    #[test]
    fn a_matched_menu_label_shows_every_entry() -> Result<(), TestError> {
        let mut app = filtered_popup_app(&FIXTURE_AVATAR, "avatar")?;
        let commands = app
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(app.world())
            .count();
        assert_eq!(
            commands, 5,
            "every command shows under a matched menu label"
        );
        Ok(())
    }

    /// A submenu is kept when its subtree carries a match, and dropped when it
    /// does not — so a hit nested one level deep is still reachable.
    #[test]
    fn a_filter_keeps_a_submenu_with_a_nested_match() -> Result<(), TestError> {
        let mut with_match = filtered_popup_app(&FIXTURE_WORLD, "sunset")?;
        let branches = with_match
            .world_mut()
            .query::<&MenuBranch>()
            .iter(with_match.world())
            .count();
        assert_eq!(branches, 1, "the Environment submenu is kept for its match");
        let top_commands = with_match
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(with_match.world())
            .count();
        assert_eq!(top_commands, 0, "no top-level World command matched");

        let mut without_match = filtered_popup_app(&FIXTURE_WORLD, "mini")?;
        let branches = without_match
            .world_mut()
            .query::<&MenuBranch>()
            .iter(without_match.world())
            .count();
        assert_eq!(
            branches, 0,
            "the submenu is dropped when nothing in it matches"
        );
        Ok(())
    }

    /// Spawn a live fixture bar (element `test-bar`) under a full menu-widget
    /// runtime, then apply the search filter `query` and settle. The bar's picks
    /// need the picking / keyboard resources the layout harness omits.
    fn filtered_bar_app(query: &str) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        enable_action_recording(&mut app);
        app.init_resource::<HoverMap>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(super::MenuWidgetPlugin);
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                super::spawn_menu_bar(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    &FIXTURE_MENU_BAR,
                    "test-bar",
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        app.insert_resource(super::MenuFilter {
            element: "test-bar",
            query: query.to_lowercase(),
        });
        settle(&mut app);
        Ok(app)
    }

    /// A term opens the **first** bar menu (in bar order) that carries a match —
    /// the leftmost, even when a later menu also matches.
    #[test]
    fn a_term_opens_the_first_matching_menu() -> Result<(), TestError> {
        // "quit" is in Avatar (first). Avatar opens, World stays closed.
        let mut app = filtered_bar_app("quit")?;
        assert!(
            find_by_name(&mut app, "menu-popup:Avatar").is_some(),
            "the first matching menu opens",
        );
        assert!(
            find_by_name(&mut app, "menu-popup:World").is_none(),
            "a non-matching (or later) menu stays closed",
        );
        Ok(())
    }

    /// When only a later menu matches, that one opens — bar order, not always the
    /// first menu.
    #[test]
    fn a_term_skips_to_a_later_matching_menu() -> Result<(), TestError> {
        // "teleport" is only in World (second); Avatar has no match.
        let mut app = filtered_bar_app("teleport")?;
        assert!(
            find_by_name(&mut app, "menu-popup:World").is_some(),
            "the first *matching* menu opens, though it is not the first menu",
        );
        assert!(
            find_by_name(&mut app, "menu-popup:Avatar").is_none(),
            "the earlier non-matching menu is left closed",
        );
        Ok(())
    }

    /// Clearing the term closes the menu the filter opened.
    #[test]
    fn clearing_the_term_closes_the_menu() -> Result<(), TestError> {
        let mut app = filtered_bar_app("quit")?;
        assert!(find_by_name(&mut app, "menu-popup:Avatar").is_some());
        app.insert_resource(super::MenuFilter {
            element: "test-bar",
            query: String::new(),
        });
        settle(&mut app);
        assert!(
            find_by_name(&mut app, "menu-popup:Avatar").is_none(),
            "an empty term closes the filter-opened menu",
        );
        Ok(())
    }

    /// A drop-down hugs its content vertically — no dead space below the last
    /// entry. Guards the taffy quirk `build_menu_popup` sidesteps (an
    /// absolutely-positioned column that stretches its children was grown far
    /// taller than its rows, worst on the one-line "(no entries yet)" menu).
    #[test]
    fn a_popup_hugs_its_content_height() -> Result<(), TestError> {
        static PLACEHOLDER: MenuDef = MenuDef {
            label: "Comm",
            items: &[MenuItemDef::Command(
                super::MenuCommand::new("(no entries yet)", "noop").enabled_when("never"),
            )],
        };
        let mut app = popup_app(&PLACEHOLDER, &[])?;
        let popup = find_by_name(&mut app, "menu-popup:Comm")
            .ok_or("the placeholder popup did not spawn")?;
        let row =
            find_by_name(&mut app, "menu-item:noop").ok_or("the placeholder row is missing")?;
        let popup_height = app
            .world()
            .entity(popup)
            .get::<bevy::ui::ComputedNode>()
            .ok_or("no computed node on the popup")?
            .size()
            .y;
        let row_height = app
            .world()
            .entity(row)
            .get::<bevy::ui::ComputedNode>()
            .ok_or("no computed node on the row")?
            .size()
            .y;
        // The popup is the row plus its own padding (4 px each side) and border
        // (1 px each side): 10 px of chrome, no dead line below.
        let expected = row_height + 10.0;
        assert!(
            (popup_height - expected).abs() < 2.0,
            "the popup should hug its one row ({expected} px), but is {popup_height} px tall — \
             dead space below the entry has crept back",
        );
        Ok(())
    }

    /// Every command row in a drop-down is the same width, so the hover highlight
    /// reads as a full-width bar rather than shrinking to each label — the width
    /// is filled by an explicit `width: 100%`, since the popup cannot use a
    /// cross-axis stretch (see [`a_popup_hugs_its_content_height`]).
    #[test]
    fn every_entry_row_is_full_width() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_AVATAR, &[])?;
        let widths: Vec<f32> = {
            let popup = find_by_name(&mut app, "menu-popup:Avatar")
                .ok_or("the Avatar popup did not spawn")?;
            let kids: Vec<Entity> = app
                .world()
                .entity(popup)
                .get::<Children>()
                .map(|c| c.iter().collect())
                .unwrap_or_default();
            kids.into_iter()
                .filter_map(|kid| {
                    let entity = app.world().entity(kid);
                    // Command rows only; a separator is a thin rule of its own and
                    // carries no `MenuEntryAction`.
                    entity.get::<MenuEntryAction>()?;
                    entity.get::<bevy::ui::ComputedNode>().map(|cn| cn.size().x)
                })
                .collect()
        };
        assert!(widths.len() >= 2, "expected several command rows");
        let first = widths.first().copied().unwrap_or(0.0);
        for width in &widths {
            assert!(
                (width - first).abs() < 1.0,
                "entry rows differ in width ({widths:?}) — the highlight would be ragged",
            );
        }
        Ok(())
    }

    /// The entity of the command line emitting `action`, if present.
    fn action_entity(app: &mut App, action: &str) -> Option<Entity> {
        app.world_mut()
            .query::<(Entity, &MenuEntryAction)>()
            .iter(app.world())
            .find(|(_, entry)| entry.action == action)
            .map(|(entity, _)| entity)
    }

    /// How many entities carry the given `Name` — for counting separators.
    fn count_named(app: &mut App, name: &str) -> usize {
        app.world_mut()
            .query::<&Name>()
            .iter(app.world())
            .filter(|entity_name| entity_name.as_str() == name)
            .count()
    }
}
