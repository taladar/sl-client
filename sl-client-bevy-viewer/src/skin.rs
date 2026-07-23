//! The viewer skin / design-token system (`viewer-ui-skin-tokens`): real CSS as
//! the skin language, on top of [`bevy_flair`].
//!
//! # Why CSS, and why `bevy_flair`
//!
//! The reference viewer's skinning is, in practice, a **design-token exercise**:
//! its skins and themes are almost entirely a `colors.xml` of named colours plus
//! named textures — *no theme overrides a layout*. So a skin is modelled here as
//! named **role tokens** (colours, textures, fonts) rather than as whole-file
//! layout replacement (the reference path that forks a 3,500-line
//! `floater_tools.xml` and then breaks every release — deliberately not copied).
//!
//! `bevy_flair` gives us a real CSS engine — selectors, pseudo-classes
//! (`:hover` / `:focus` / `:active`), `var()` custom-property tokens,
//! `@keyframes`, `@import`, and hot-reloaded `.css` — a far better skin language
//! than XUI and the natural home for the tokens. It is used here as:
//!
//! - **the token layer** — a skin is a `:root { --role: value; … }` block of
//!   abstract role tokens with direct values (no literal-colour-named palette
//!   tier); every panel references a role token, never an inline colour. A
//!   **theme overlay** is a later `@import`-ed skin that redefines a *subset* of
//!   those tokens — the CSS cascade *is* the reference's id-keyed merge.
//! - **the paint layer** — `background-color` / `border-color` / `color` /
//!   `font-*` / `-bevy-image` apply straight onto the node's paint components;
//!   these carry no handedness, so `bevy_flair`'s native (physical) support is
//!   used as-is.
//!
//! # Bidi: logical properties through the shipped resolver
//!
//! `bevy_flair`'s built-in box properties are **physical** (`margin-left`,
//! `inset`, `border-top-left-radius`) and write straight onto `Node`, which
//! would not mirror under an RTL locale and would fight the widget scaffold's
//! [`crate::ui`] logical box model. So this module goes beyond the reference:
//!
//! - It registers a set of **logical** CSS box + corner properties
//!   (`margin-inline-start`, `padding-block-end`, `inset-inline-start`,
//!   `border-inline-start-width`, `border-start-start-radius`, …) that
//!   `bevy_flair` parses, cascades and `var()`-resolves into the flat
//!   [`SkinMargin`] / [`SkinPadding`] / [`SkinBorder`] / [`SkinInset`] /
//!   [`SkinRadius`] components. [`resolve_skin_boxes`] then folds those into the
//!   physical `Node` against the live [`UiDirection`], reusing the same
//!   [`LogicalRect::resolve`] mirror the scaffold's own boxes use — so a skin's
//!   `margin-inline-start` mirrors to the right edge under RTL for free.
//! - The **physical** originals (`margin-left`, `left`, `border-top-left-radius`,
//!   …) are **banned**: [`scan_banned_properties`] flags any of them, and the
//!   test suite fails the build if a shipped skin uses one. A skin author writes
//!   only the logical names.
//!
//! # i18n-aware skins
//!
//! The active locale is bridged onto the [`UiRoot`] as CSS **attributes**
//! (`dir="rtl"`, `lang="ja"`) by [`sync_skin_attributes`], so a skin or overlay
//! can be locale-conditional with an attribute selector
//! (`:root[lang="ja"] { … }`) — and the culture-colour and colour-blind overlays
//! (`viewer-i18n-cultural-color-meanings` / `viewer-i18n-colorblind-accessibility`)
//! hook in the same way, through a `[data-culture]` / `[data-vision]` attribute.
//! Translated *strings* stay in Fluent; theme-authored localized labels/numbers
//! in CSS are a separate follow-up (`viewer-ui-skin-l10n-functions`), for which
//! the loader here leaves a preprocess seam.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::UiSystems;
// `CssPropertyRegistry`, `RegisterComponentPropertiesExt` and
// `ReflectStructPropertyRefExt` all arrive through the prelude
// (`pub use bevy_flair_core::*`).
use bevy_flair::prelude::*;
use bevy_flair::style::StyleSystems;

use crate::i18n::UiLocale;
use crate::ui::{LogicalRect, UiDirection, UiRoot, UiScaffoldSystems};

/// The asset-path directory (under the Bevy asset root) that holds the skins.
const SKINS_DIR: &str = "skins";

/// The file name of a skin's base stylesheet (no theme selected).
const SKIN_BASE_FILE: &str = "skin.css";

/// The subdirectory, under a skin, that holds its theme overlays.
const THEMES_DIR: &str = "themes";

/// The default skin id, used when neither the CLI nor the environment selects
/// one. Matches a directory under [`SKINS_DIR`].
pub(crate) const DEFAULT_SKIN: &str = "graphite";

/// The skin ids that ship with the viewer, in switcher-cycle order. Each is a
/// directory under `assets/skins/` holding a [`SKIN_BASE_FILE`].
pub(crate) const SKINS: &[&str] = &["graphite", "azure"];

/// The theme overlays that ship, keyed loosely by skin: a `(skin, theme)` pair
/// names `assets/skins/<skin>/themes/<theme>.css`. `None` in the switcher cycle
/// means "the skin's own base, no overlay".
pub(crate) const THEMES: &[(&str, &str)] = &[("graphite", "dark")];

/// The environment variable that seeds the initial [`SkinSelection`] skin id,
/// for the offline screenshot harness. The CLI `--skin` flag is the
/// user-facing selector; this is the debug-only override.
const SKIN_ENV: &str = "SL_VIEWER_SKIN";

/// The environment variable that seeds the initial theme overlay id.
const THEME_ENV: &str = "SL_VIEWER_THEME";

/// Which skin and (optional) theme overlay the UI is currently wearing.
///
/// Written by the CLI at start-up and by the gallery's switcher at runtime;
/// [`apply_skin_selection`] reloads the [`UiRoot`]'s stylesheet whenever it
/// changes, so a flip re-styles the whole tree live.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkinSelection {
    /// The skin id — a directory under `assets/skins/`.
    pub(crate) skin: String,
    /// The theme overlay id, or `None` for the skin's own base.
    pub(crate) theme: Option<String>,
}

impl Default for SkinSelection {
    /// The default selection: [`DEFAULT_SKIN`] with no overlay.
    fn default() -> Self {
        Self {
            skin: DEFAULT_SKIN.to_owned(),
            theme: None,
        }
    }
}

impl SkinSelection {
    /// The initial selection, from the CLI values if given, else the
    /// [`SKIN_ENV`] / [`THEME_ENV`] overrides, else the default.
    pub(crate) fn resolve(skin: Option<String>, theme: Option<String>) -> Self {
        let skin = skin
            .or_else(|| std::env::var(SKIN_ENV).ok())
            .unwrap_or_else(|| DEFAULT_SKIN.to_owned());
        let theme = theme.or_else(|| std::env::var(THEME_ENV).ok());
        Self { skin, theme }
    }

    /// The asset path of the stylesheet this selection loads: the theme overlay
    /// when one is selected (it `@import`s its skin base), else the skin base.
    fn asset_path(&self) -> String {
        match &self.theme {
            Some(theme) => format!("{SKINS_DIR}/{}/{THEMES_DIR}/{theme}.css", self.skin),
            None => format!("{SKINS_DIR}/{}/{SKIN_BASE_FILE}", self.skin),
        }
    }

    /// Advance to the next shipped [`SKINS`] skin, wrapping, and drop any theme
    /// overlay (themes are skin-specific). Drives the gallery's skin switcher.
    pub(crate) fn cycle_skin(&mut self) {
        let current = SKINS.iter().position(|candidate| *candidate == self.skin);
        let next_index = next_in_cycle(current, SKINS.len());
        if let Some(next) = SKINS.get(next_index) {
            (*next).clone_into(&mut self.skin);
        }
        self.theme = None;
    }

    /// Advance the theme overlay through the current skin's [`THEMES`] and back
    /// to `None` (the skin's own base). Drives the gallery's theme switcher.
    pub(crate) fn cycle_theme(&mut self) {
        // The cycle for this skin: no overlay, then each shipped theme, repeat.
        let mut options: Vec<Option<&str>> = vec![None];
        for (skin, theme) in THEMES {
            if *skin == self.skin {
                options.push(Some(theme));
            }
        }
        let current = options
            .iter()
            .position(|option| *option == self.theme.as_deref());
        let next_index = next_in_cycle(current, options.len());
        if let Some(next) = options.get(next_index) {
            self.theme = next.map(str::to_owned);
        }
    }

    /// A short human label of this selection for the switcher, e.g.
    /// `graphite / dark` or `graphite / (base)`.
    pub(crate) fn label(&self) -> String {
        match &self.theme {
            Some(theme) => format!("{} / {theme}", self.skin),
            None => format!("{} / (base)", self.skin),
        }
    }
}

/// The next index in a wrapping cycle of `len` items: the item after `current`,
/// or `0` when `current` is the last, absent, or the cycle is empty. Avoids the
/// `%` operator, which `arithmetic_side_effects` denies.
const fn next_in_cycle(current: Option<usize>, len: usize) -> usize {
    match current {
        Some(index) if index.saturating_add(1) < len => index.saturating_add(1),
        _other => 0,
    }
}

/// The viewer skin plugin: stands up `bevy_flair`, registers the logical box /
/// corner properties, loads the selected skin onto the [`UiRoot`], and keeps the
/// locale attributes in step. See the [module documentation](self).
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewerSkinPlugin;

impl Plugin for ViewerSkinPlugin {
    fn build(&self, app: &mut App) {
        // The CSS engine. Brought up before our own property registration so its
        // `PropertyRegistry` / `CssPropertyRegistry` resources exist to extend.
        app.add_plugins(FlairPlugin);
        register_logical_properties(app);
        register_caret_properties(app);
        app.init_resource::<SkinSelection>()
            .add_systems(
                Startup,
                // After the root exists, so there is something to dress.
                apply_skin_selection.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    // Re-dress the root when the switcher flips the selection.
                    apply_skin_selection.run_if(resource_changed::<SkinSelection>),
                    sync_skin_attributes,
                    // Tag each focusable widget so the skin's focus-ring rule
                    // reaches it (`viewer-ui-focus-ring-visible`).
                    stamp_focus_ring_class,
                    // Tag each editable text field so the skin's caret /
                    // selection / focused-field rules reach it (R28).
                    stamp_text_field_class,
                ),
            )
            .add_systems(
                PostUpdate,
                (invalidate_skin_boxes, resolve_skin_boxes)
                    .chain()
                    // After `bevy_flair` has written the logical box components
                    // for this frame, and before the layout pass reads `Node`.
                    .after(StyleSystems::ApplyComputedProperties)
                    .before(UiSystems::Layout),
            );
    }
}

/// The CSS class the scaffold tags every focusable widget with, so the single
/// `.sk-focusable:focus-visible` rule in `common.css` draws the keyboard focus
/// ring on it. See [`stamp_focus_ring_class`].
const FOCUSABLE_CLASS: &str = "sk-focusable";

/// Tag every focusable widget with [`FOCUSABLE_CLASS`], so the skin's
/// `.sk-focusable:focus-visible` outline rings it whenever keyboard focus lands
/// there (`viewer-ui-focus-ring-visible`).
///
/// A widget is focusable exactly when it carries a `TabIndex` — the same thing
/// `bevy_input_focus`'s tab navigation walks — so keying off `Added<TabIndex>`
/// covers every one, present and future, with no per-widget wiring: a new
/// focusable widget (a menu-bar button, an inventory row, a demo control) gets
/// the ring for free the frame after it spawns. This is the one place the focus
/// ring is wired; the CSS is the one place it is drawn.
///
/// The class is merged into whatever [`ClassList`] the widget already carries (a
/// menu-bar button keeps its `sk-menu-bar-item`), or a fresh list is inserted
/// when it has none. `Added<TabIndex>` also fires when a parked index is
/// restored as a panel reopens ([`crate::ui::apply_panel_visibility`]); the
/// `contains` guard makes that re-stamp a no-op.
fn stamp_focus_ring_class(
    mut commands: Commands,
    mut focusable: Query<(Entity, Option<&mut ClassList>), Added<TabIndex>>,
) {
    for (entity, class_list) in &mut focusable {
        match class_list {
            Some(mut list) => {
                if !list.contains(FOCUSABLE_CLASS) {
                    list.add(FOCUSABLE_CLASS);
                }
            }
            None => {
                commands
                    .entity(entity)
                    .insert(ClassList::new_with_classes([FOCUSABLE_CLASS]));
            }
        }
    }
}

/// The CSS class the scaffold tags every editable text field with (R28), so the
/// skin's caret / selection colour rule (`.sk-text-field`) and its any-focus
/// ring (`.sk-text-field:focus`) reach every editor. See
/// [`stamp_text_field_class`].
const TEXT_FIELD_CLASS: &str = "sk-text-field";

/// Tag every editable text field with [`TEXT_FIELD_CLASS`] (R28), the caret /
/// selection counterpart of [`stamp_focus_ring_class`]: keying off
/// `Added<EditableText>` covers every editor, present and future, with no
/// per-widget wiring — the caret colours and the focused-field ring come from
/// the one `common.css` rule pair. The class is merged into whatever
/// [`ClassList`] the field already carries (a search field keeps its
/// `sk-search-field`), or a fresh list is inserted when it has none.
fn stamp_text_field_class(
    mut commands: Commands,
    mut fields: Query<(Entity, Option<&mut ClassList>), Added<EditableText>>,
) {
    for (entity, class_list) in &mut fields {
        match class_list {
            Some(mut list) => {
                if !list.contains(TEXT_FIELD_CLASS) {
                    list.add(TEXT_FIELD_CLASS);
                }
            }
            None => {
                commands
                    .entity(entity)
                    .insert(ClassList::new_with_classes([TEXT_FIELD_CLASS]));
            }
        }
    }
}

/// The skin-driven text-caret and selection colours of one editable text field
/// (R28), written by the `caret-color` / `selection-color` /
/// `unfocused-selection-color` CSS properties (the `.sk-text-field` rule in
/// `common.css`, whose values are the `--caret` / `--selection` /
/// `--selection-unfocused` role tokens) and folded into the field's
/// [`bevy::text::TextCursorStyle`] by
/// [`crate::ui_text_input::drive_caret_blink`].
///
/// This component exists because `TextCursorStyle` itself is not reflectable,
/// so `bevy_flair` cannot drive it directly — the same shim pattern as the
/// logical box components above. Bevy's default caret colour is a
/// light-theme slate that is invisible on our near-black field backgrounds,
/// which was the heart of R28; the defaults here are the visible fallback for
/// a field the skin has not styled (the reference default: the text colour).
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinTextCaret {
    /// The caret (text cursor) colour.
    pub(crate) caret: Color,
    /// The background colour of selected text while the field is focused.
    pub(crate) selection: Color,
    /// The background colour of selected text while the field is unfocused.
    pub(crate) selection_unfocused: Color,
}

impl Default for SkinTextCaret {
    /// Visible-on-dark fallbacks for an unskinned field: a white caret (the
    /// field text colour) and translucent blue-grey selections.
    fn default() -> Self {
        Self {
            caret: Color::WHITE,
            selection: Color::srgba(0.30, 0.55, 0.90, 0.45),
            selection_unfocused: Color::srgba(0.45, 0.52, 0.62, 0.35),
        }
    }
}

/// Register the text-caret CSS properties (R28) on the `bevy_flair` registry,
/// mapping `caret-color` (the standard CSS property) and the two selection
/// colours onto [`SkinTextCaret`]'s fields. Runs in `build`, before the CSS
/// asset loader snapshots the registry at plugin `finish`.
fn register_caret_properties(app: &mut App) {
    app.register_component_properties::<SkinTextCaret>();
    let css = app.world().resource::<CssPropertyRegistry>();
    css.register_property("caret-color", SkinTextCaret::property_field_ref("caret"));
    css.register_property(
        "selection-color",
        SkinTextCaret::property_field_ref("selection"),
    );
    css.register_property(
        "unfocused-selection-color",
        SkinTextCaret::property_field_ref("selection_unfocused"),
    );
}

// ---------------------------------------------------------------------------
// Logical box + corner properties.
//
// Flat, reflectable components `bevy_flair` writes CSS logical properties into,
// then `resolve_skin_boxes` folds into the physical `Node` against the live
// direction. Kept separate from the scaffold's `LogicalMargin(LogicalRect)`
// newtypes (which are the *code*-facing API) so `bevy_flair` can address each
// edge as an individual `Val` property; the two never share a `Node` field on
// one entity — a given box is owned by CSS *or* by code, not both.
// ---------------------------------------------------------------------------

/// A node's margin in logical (writing-mode-relative) edges, written by the
/// `margin-inline-start` / `margin-inline-end` / `margin-block-start` /
/// `margin-block-end` CSS properties and folded into `Node::margin`.
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinMargin {
    /// The leading inline edge (left under LTR, right under RTL).
    inline_start: Val,
    /// The trailing inline edge.
    inline_end: Val,
    /// The leading block edge — the top.
    block_start: Val,
    /// The trailing block edge — the bottom.
    block_end: Val,
}

impl Default for SkinMargin {
    /// Unset edges are zero, not `Val::Auto` — an auto margin would centre or
    /// push the node, which is never what an unset skin margin means.
    fn default() -> Self {
        Self {
            inline_start: Val::ZERO,
            inline_end: Val::ZERO,
            block_start: Val::ZERO,
            block_end: Val::ZERO,
        }
    }
}

impl SkinMargin {
    /// This margin as the direction-independent [`LogicalRect`], for
    /// [`LogicalRect::resolve`].
    const fn rect(self) -> LogicalRect {
        LogicalRect {
            inline_start: self.inline_start,
            inline_end: self.inline_end,
            block_start: self.block_start,
            block_end: self.block_end,
        }
    }
}

/// A node's padding in logical edges, written by the `padding-inline-*` /
/// `padding-block-*` CSS properties and folded into `Node::padding`.
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinPadding {
    /// The leading inline edge.
    inline_start: Val,
    /// The trailing inline edge.
    inline_end: Val,
    /// The leading block edge — the top.
    block_start: Val,
    /// The trailing block edge — the bottom.
    block_end: Val,
}

impl Default for SkinPadding {
    /// Unset padding edges are zero.
    fn default() -> Self {
        Self {
            inline_start: Val::ZERO,
            inline_end: Val::ZERO,
            block_start: Val::ZERO,
            block_end: Val::ZERO,
        }
    }
}

impl SkinPadding {
    /// This padding as a [`LogicalRect`].
    const fn rect(self) -> LogicalRect {
        LogicalRect {
            inline_start: self.inline_start,
            inline_end: self.inline_end,
            block_start: self.block_start,
            block_end: self.block_end,
        }
    }
}

/// A node's border widths in logical edges, written by the
/// `border-inline-*-width` / `border-block-*-width` CSS properties and folded
/// into `Node::border`. (The border *colour* is `border-color`, handled by
/// `bevy_flair` natively — colour has no handedness.)
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinBorder {
    /// The leading inline edge width.
    inline_start: Val,
    /// The trailing inline edge width.
    inline_end: Val,
    /// The leading block edge width — the top.
    block_start: Val,
    /// The trailing block edge width — the bottom.
    block_end: Val,
}

impl Default for SkinBorder {
    /// Unset border widths are zero.
    fn default() -> Self {
        Self {
            inline_start: Val::ZERO,
            inline_end: Val::ZERO,
            block_start: Val::ZERO,
            block_end: Val::ZERO,
        }
    }
}

impl SkinBorder {
    /// These border widths as a [`LogicalRect`].
    const fn rect(self) -> LogicalRect {
        LogicalRect {
            inline_start: self.inline_start,
            inline_end: self.inline_end,
            block_start: self.block_start,
            block_end: self.block_end,
        }
    }
}

/// A node's inset (its `left` / `right` / `top` / `bottom` position) in logical
/// edges, written by the `inset-inline-*` / `inset-block-*` CSS properties and
/// folded into the four `Node` inset fields.
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinInset {
    /// The leading inline edge.
    inline_start: Val,
    /// The trailing inline edge.
    inline_end: Val,
    /// The leading block edge — the top.
    block_start: Val,
    /// The trailing block edge — the bottom.
    block_end: Val,
}

impl Default for SkinInset {
    /// Unset inset edges are `Val::Auto` — "leave this edge to flow", not pinned
    /// to the container (a zero inset would stretch the node to the edge).
    fn default() -> Self {
        Self {
            inline_start: Val::Auto,
            inline_end: Val::Auto,
            block_start: Val::Auto,
            block_end: Val::Auto,
        }
    }
}

impl SkinInset {
    /// This inset as a [`LogicalRect`].
    const fn rect(self) -> LogicalRect {
        LogicalRect {
            inline_start: self.inline_start,
            inline_end: self.inline_end,
            block_start: self.block_start,
            block_end: self.block_end,
        }
    }
}

/// A node's corner radii in logical corners, written by the
/// `border-start-start-radius` / `border-start-end-radius` /
/// `border-end-start-radius` / `border-end-end-radius` CSS properties and folded
/// into `Node::border_radius`. Under RTL the two inline sides of each corner
/// swap, so an asymmetric (tab / bubble) corner mirrors.
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub(crate) struct SkinRadius {
    /// The block-start, inline-start corner (top-leading).
    start_start: Val,
    /// The block-start, inline-end corner (top-trailing).
    start_end: Val,
    /// The block-end, inline-start corner (bottom-leading).
    end_start: Val,
    /// The block-end, inline-end corner (bottom-trailing).
    end_end: Val,
}

impl Default for SkinRadius {
    /// Unset corners are square (zero radius).
    fn default() -> Self {
        Self {
            start_start: Val::ZERO,
            start_end: Val::ZERO,
            end_start: Val::ZERO,
            end_end: Val::ZERO,
        }
    }
}

impl SkinRadius {
    /// Resolve these logical corners into a physical [`BorderRadius`] for the
    /// given direction: under RTL the leading corners land on the right.
    const fn resolve(self, direction: UiDirection) -> BorderRadius {
        let (top_left, top_right, bottom_left, bottom_right) = if direction.is_rtl() {
            (
                self.start_end,
                self.start_start,
                self.end_end,
                self.end_start,
            )
        } else {
            (
                self.start_start,
                self.start_end,
                self.end_start,
                self.end_end,
            )
        };
        BorderRadius {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }
}

/// Register the logical box + corner CSS properties on the `bevy_flair`
/// registry, mapping each logical name onto a field of the flat [`SkinMargin`] /
/// … components. Runs in `build`, before the CSS asset loader snapshots the
/// registry at plugin `finish`.
fn register_logical_properties(app: &mut App) {
    app.register_component_properties::<SkinMargin>();
    app.register_component_properties::<SkinPadding>();
    app.register_component_properties::<SkinBorder>();
    app.register_component_properties::<SkinInset>();
    app.register_component_properties::<SkinRadius>();

    let css = app.world().resource::<CssPropertyRegistry>();

    css.register_property(
        "margin-inline-start",
        SkinMargin::property_field_ref("inline_start"),
    );
    css.register_property(
        "margin-inline-end",
        SkinMargin::property_field_ref("inline_end"),
    );
    css.register_property(
        "margin-block-start",
        SkinMargin::property_field_ref("block_start"),
    );
    css.register_property(
        "margin-block-end",
        SkinMargin::property_field_ref("block_end"),
    );

    css.register_property(
        "padding-inline-start",
        SkinPadding::property_field_ref("inline_start"),
    );
    css.register_property(
        "padding-inline-end",
        SkinPadding::property_field_ref("inline_end"),
    );
    css.register_property(
        "padding-block-start",
        SkinPadding::property_field_ref("block_start"),
    );
    css.register_property(
        "padding-block-end",
        SkinPadding::property_field_ref("block_end"),
    );

    css.register_property(
        "border-inline-start-width",
        SkinBorder::property_field_ref("inline_start"),
    );
    css.register_property(
        "border-inline-end-width",
        SkinBorder::property_field_ref("inline_end"),
    );
    css.register_property(
        "border-block-start-width",
        SkinBorder::property_field_ref("block_start"),
    );
    css.register_property(
        "border-block-end-width",
        SkinBorder::property_field_ref("block_end"),
    );

    css.register_property(
        "inset-inline-start",
        SkinInset::property_field_ref("inline_start"),
    );
    css.register_property(
        "inset-inline-end",
        SkinInset::property_field_ref("inline_end"),
    );
    css.register_property(
        "inset-block-start",
        SkinInset::property_field_ref("block_start"),
    );
    css.register_property(
        "inset-block-end",
        SkinInset::property_field_ref("block_end"),
    );

    css.register_property(
        "border-start-start-radius",
        SkinRadius::property_field_ref("start_start"),
    );
    css.register_property(
        "border-start-end-radius",
        SkinRadius::property_field_ref("start_end"),
    );
    css.register_property(
        "border-end-start-radius",
        SkinRadius::property_field_ref("end_start"),
    );
    css.register_property(
        "border-end-end-radius",
        SkinRadius::property_field_ref("end_end"),
    );
}

/// The nodes [`resolve_skin_boxes`] has work for: any carrying a skin box that
/// changed since it last ran (or all of them the frame the direction flips —
/// see [`invalidate_skin_boxes`]).
type ChangedSkinBoxes<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static mut Node,
        Option<&'static SkinMargin>,
        Option<&'static SkinPadding>,
        Option<&'static SkinBorder>,
        Option<&'static SkinInset>,
        Option<&'static SkinRadius>,
    ),
    Or<(
        Changed<SkinMargin>,
        Changed<SkinPadding>,
        Changed<SkinBorder>,
        Changed<SkinInset>,
        Changed<SkinRadius>,
    )>,
>;

/// Fold each node's skin box components into the physical `Node` fields the
/// layout reads, against the live [`UiDirection`] — the CSS-driven twin of the
/// scaffold's `resolve_logical_boxes`. Writes through change detection only on a
/// real difference, so a settled UI does not re-trigger layout every frame.
fn resolve_skin_boxes(direction: Res<UiDirection>, mut nodes: ChangedSkinBoxes) {
    for (mut node, margin, padding, border, inset, radius) in &mut nodes {
        if let Some(margin) = margin {
            let resolved = margin.rect().resolve(*direction);
            if node.margin != resolved {
                node.margin = resolved;
            }
        }
        if let Some(padding) = padding {
            let resolved = padding.rect().resolve(*direction);
            if node.padding != resolved {
                node.padding = resolved;
            }
        }
        if let Some(border) = border {
            let resolved = border.rect().resolve(*direction);
            if node.border != resolved {
                node.border = resolved;
            }
        }
        if let Some(inset) = inset {
            let resolved = inset.rect().resolve(*direction);
            if node.left != resolved.left {
                node.left = resolved.left;
            }
            if node.right != resolved.right {
                node.right = resolved.right;
            }
            if node.top != resolved.top {
                node.top = resolved.top;
            }
            if node.bottom != resolved.bottom {
                node.bottom = resolved.bottom;
            }
        }
        if let Some(radius) = radius {
            let resolved = radius.resolve(*direction);
            if node.border_radius != resolved {
                node.border_radius = resolved;
            }
        }
    }
}

/// Mark every skin box dirty when [`UiDirection`] flips, so [`resolve_skin_boxes`]
/// — otherwise driven by change detection on the components — re-resolves the
/// whole tree against the new direction. Mirrors the scaffold's
/// `invalidate_logical_boxes`.
fn invalidate_skin_boxes(
    direction: Res<UiDirection>,
    mut margins: Query<&mut SkinMargin>,
    mut paddings: Query<&mut SkinPadding>,
    mut borders: Query<&mut SkinBorder>,
    mut insets: Query<&mut SkinInset>,
    mut radii: Query<&mut SkinRadius>,
) {
    if !direction.is_changed() {
        return;
    }
    for mut margin in &mut margins {
        margin.set_changed();
    }
    for mut padding in &mut paddings {
        padding.set_changed();
    }
    for mut border in &mut borders {
        border.set_changed();
    }
    for mut inset in &mut insets {
        inset.set_changed();
    }
    for mut radius in &mut radii {
        radius.set_changed();
    }
}

/// Start-up and on-change system: attach the selected skin stylesheet to the
/// [`UiRoot`], so its whole subtree is styled. Children inherit the stylesheet,
/// so this one [`Styled`] dresses every panel.
fn apply_skin_selection(
    mut commands: Commands,
    selection: Res<SkinSelection>,
    asset_server: Res<AssetServer>,
    root: Res<UiRoot>,
) {
    let path = selection.asset_path();
    debug!("dressing UiRoot {:?} in skin stylesheet {path}", root.0);
    let handle: Handle<StyleSheet> = asset_server.load(&path);
    commands.entity(root.0).insert(Styled::new(handle));
}

/// Bridge the active locale onto the [`UiRoot`] as CSS attributes, so a skin or
/// overlay can select on it (`:root[dir="rtl"]`, `:root[lang="ja"]`). This is
/// how locale-conditional token values (fonts, and the culture / colour-blind
/// overlays) are expressed in CSS without leaking layout handedness.
fn sync_skin_attributes(
    mut commands: Commands,
    direction: Res<UiDirection>,
    // Optional so the skin system does not hard-require the i18n plugin: the
    // gallery runs without it, and then only `dir` is bridged.
    locale: Option<Res<UiLocale>>,
    root: Res<UiRoot>,
    mut attributes: Query<&mut AttributeList>,
) {
    let locale_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    if !direction.is_changed() && !locale_changed {
        return;
    }
    let dir = if direction.is_rtl() { "rtl" } else { "ltr" };
    // The active language tag, or `und` (undetermined) when no locale plugin is
    // present — a valid CSS attribute value the selectors can still match.
    let lang = locale.as_ref().map_or_else(
        || "und".to_owned(),
        |locale| locale.lang.language.as_str().to_owned(),
    );
    if let Ok(mut list) = attributes.get_mut(root.0) {
        list.set_attribute("dir", dir);
        list.set_attribute("lang", lang);
    } else {
        // Not present yet the first time this runs; insert a populated list so
        // the selectors have something to match. Later runs take the branch
        // above and mutate it in place.
        let mut list = AttributeList::new();
        list.set_attribute("dir", dir);
        list.set_attribute("lang", lang);
        commands.entity(root.0).insert(list);
    }
}

/// The physical CSS box / corner properties a skin must never use: they write
/// straight onto `Node` and would not mirror under RTL. A skin author writes the
/// logical name instead — the mapping is in the error the tests report.
///
/// Enforced today at build time by the test suite (a shipped skin that uses one
/// fails the build). Gated to test builds because that is its only consumer for
/// now; the `viewer-ui-skin-l10n-functions` / user-skin follow-up will lift the
/// gate for a runtime validator of user-authored skins.
#[cfg(test)]
const BANNED_PHYSICAL_PROPERTIES: &[&str] = &[
    "margin-left",
    "margin-right",
    "padding-left",
    "padding-right",
    "border-left-width",
    "border-right-width",
    "border-left-color",
    "border-right-color",
    "left",
    "right",
    "inset",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
];

/// The logical replacement a banned physical property should be rewritten to,
/// for the error message. `None` when there is no single logical equivalent
/// (`inset` is a shorthand; use the four `inset-*` longhands).
#[cfg(test)]
fn logical_replacement(physical: &str) -> Option<&'static str> {
    let replacement = match physical {
        "margin-left" | "margin-right" => "margin-inline-start / margin-inline-end",
        "padding-left" | "padding-right" => "padding-inline-start / padding-inline-end",
        "border-left-width" | "border-right-width" => {
            "border-inline-start-width / border-inline-end-width"
        }
        "border-left-color" | "border-right-color" => "border-color (a colour has no handedness)",
        "left" | "right" => "inset-inline-start / inset-inline-end",
        "inset" => "the four inset-inline-* / inset-block-* longhands",
        "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-left-radius"
        | "border-bottom-right-radius" => {
            "border-start-start-radius / -start-end / -end-start / -end-end"
        }
        _other => return None,
    };
    Some(replacement)
}

/// A banned physical property found in a skin stylesheet: which property, and
/// the 1-based line it is on.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct BannedProperty {
    /// The offending physical property name.
    property: String,
    /// The 1-based source line it appears on.
    line: usize,
}

/// Scan CSS source for any [`BANNED_PHYSICAL_PROPERTIES`] used as a declaration.
///
/// A deliberately simple line scanner rather than a full parser: it looks for a
/// banned name at the head of a `name: value;` declaration (optionally after
/// whitespace), which is all a shipped skin can contain. `var(--…)` names and
/// selectors are not declarations, so they are not matched. Used by the test
/// suite to fail the build if a skin reaches for a physical box property, and
/// available for a future runtime validator of user-authored skins.
#[cfg(test)]
fn scan_banned_properties(css: &str) -> Vec<BannedProperty> {
    let mut findings = Vec::new();
    for (index, raw_line) in css.lines().enumerate() {
        // A declaration is `name : value`. Take the text before the first colon
        // and trim it; a selector or at-rule has no bare `property:` head that
        // matches a banned name exactly.
        let Some((head, _rest)) = raw_line.split_once(':') else {
            continue;
        };
        let name = head.trim();
        if BANNED_PHYSICAL_PROPERTIES.contains(&name) {
            findings.push(BannedProperty {
                property: name.to_owned(),
                line: index.saturating_add(1),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::{
        BANNED_PHYSICAL_PROPERTIES, FOCUSABLE_CLASS, SKINS, SkinMargin, SkinRadius, SkinSelection,
        THEMES, UiDirection, invalidate_skin_boxes, logical_replacement, resolve_skin_boxes,
        scan_banned_properties, stamp_focus_ring_class,
    };
    use bevy::input_focus::tab_navigation::TabIndex;
    use bevy::prelude::*;
    use bevy_flair::style::components::ClassList;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    /// A boxed error so tests can use `?` instead of `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The absolute path of the shipped skins directory.
    fn skins_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("skins")
    }

    /// Every shipped skin `.css` — the base of each skin plus every theme
    /// overlay — must be free of the banned physical box properties. This is the
    /// build-time enforcement of "no physical left/right in a skin".
    #[test]
    fn no_shipped_skin_uses_a_banned_property() -> Result<(), TestError> {
        let mut checked = 0_usize;
        for entry in walk_css(&skins_dir())? {
            let css = fs_err::read_to_string(&entry)?;
            let findings = scan_banned_properties(&css);
            assert!(
                findings.is_empty(),
                "{}: uses banned physical properties {findings:?}; write the logical name instead",
                entry.display()
            );
            checked = checked.saturating_add(1);
        }
        assert!(checked > 0, "no skin css files were found to check");
        Ok(())
    }

    /// Collect every `.css` file under a directory tree.
    fn walk_css(dir: &std::path::Path) -> Result<Vec<PathBuf>, TestError> {
        let mut out = Vec::new();
        for entry in fs_err::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                out.extend(walk_css(&path)?);
            } else if path.extension().is_some_and(|ext| ext == "css") {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// The scanner flags a banned declaration but not a `var()` reference or a
    /// legitimate logical property with a similar name.
    #[test]
    fn scanner_flags_only_real_declarations() {
        let css = "\
.panel {\n\
  margin-left: 4px;\n\
  margin-inline-start: 8px;\n\
  color: var(--text);\n\
}\n";
        let findings = scan_banned_properties(css);
        assert_eq!(
            findings.len(),
            1,
            "only margin-left is banned: {findings:?}"
        );
        assert_eq!(
            findings.first().map(|f| f.property.as_str()),
            Some("margin-left")
        );
        assert_eq!(findings.first().map(|f| f.line), Some(2));
    }

    /// Every banned property has a logical replacement suggestion for its error.
    #[test]
    fn every_banned_property_has_a_replacement_hint() {
        for physical in BANNED_PHYSICAL_PROPERTIES {
            assert!(
                logical_replacement(physical).is_some(),
                "{physical} has no logical replacement hint"
            );
        }
    }

    /// A logical corner set mirrors under RTL: the leading (start) corners move
    /// to the right side.
    #[test]
    fn corners_mirror_under_rtl() {
        let radius = SkinRadius {
            start_start: Val::Px(10.0),
            start_end: Val::Px(0.0),
            end_start: Val::Px(0.0),
            end_end: Val::Px(0.0),
        };
        assert_eq!(
            radius.resolve(UiDirection::Ltr),
            BorderRadius {
                top_left: Val::Px(10.0),
                top_right: Val::Px(0.0),
                bottom_left: Val::Px(0.0),
                bottom_right: Val::Px(0.0),
            }
        );
        assert_eq!(
            radius.resolve(UiDirection::Rtl),
            BorderRadius {
                // The leading top corner is now on the right.
                top_left: Val::Px(0.0),
                top_right: Val::Px(10.0),
                bottom_left: Val::Px(0.0),
                bottom_right: Val::Px(0.0),
            }
        );
    }

    /// A CSS-driven skin margin mirrors onto the physical `Node` under RTL,
    /// exactly like the scaffold's own logical boxes — the proof that the
    /// registered logical box properties feed the shipped bidi resolver rather
    /// than writing a physical edge. Drives the real resolver systems through a
    /// minimal `App`, as the scaffold's own box test does.
    #[test]
    fn a_skin_margin_mirrors_under_rtl() -> Result<(), TestError> {
        for (direction, want_left, want_right) in [
            (UiDirection::Ltr, Val::Px(8.0), Val::ZERO),
            (UiDirection::Rtl, Val::ZERO, Val::Px(8.0)),
        ] {
            let mut app = App::new();
            app.insert_resource(direction)
                .add_systems(Update, (invalidate_skin_boxes, resolve_skin_boxes).chain());
            let node = app
                .world_mut()
                .spawn((
                    Node::default(),
                    SkinMargin {
                        inline_start: Val::Px(8.0),
                        ..SkinMargin::default()
                    },
                ))
                .id();
            app.update();
            let node = app
                .world()
                .get::<Node>(node)
                .ok_or("the spawned node lost its `Node`")?;
            assert_eq!(
                node.margin.left, want_left,
                "{direction:?}: leading margin -> left"
            );
            assert_eq!(
                node.margin.right, want_right,
                "{direction:?}: leading margin -> right under RTL"
            );
        }
        Ok(())
    }

    /// The scaffold tags every focusable widget (one carrying a `TabIndex`) with
    /// [`FOCUSABLE_CLASS`], so the skin's `.sk-focusable:focus-visible` outline
    /// reaches it: a bare widget gains a fresh class list, a widget that already
    /// carries classes keeps them and gains this one too, and a non-focusable
    /// entity is left untouched.
    #[test]
    fn stamp_tags_every_focusable_widget() -> Result<(), TestError> {
        let mut app = App::new();
        app.add_systems(Update, stamp_focus_ring_class);

        let bare = app.world_mut().spawn(TabIndex(0)).id();
        let classed = app
            .world_mut()
            .spawn((
                TabIndex(0),
                ClassList::new_with_classes(["sk-menu-bar-item"]),
            ))
            .id();
        let plain = app.world_mut().spawn_empty().id();

        app.update();

        let bare_classes = app
            .world()
            .get::<ClassList>(bare)
            .ok_or("a bare focusable widget was not given a class list")?;
        assert!(
            bare_classes.contains(FOCUSABLE_CLASS),
            "a bare focusable widget must gain the focus-ring class"
        );

        let classed_classes = app
            .world()
            .get::<ClassList>(classed)
            .ok_or("a classed focusable widget lost its class list")?;
        assert!(
            classed_classes.contains("sk-menu-bar-item"),
            "an existing class must be preserved when the focus-ring class is added"
        );
        assert!(
            classed_classes.contains(FOCUSABLE_CLASS),
            "the focus-ring class must be merged in alongside existing classes"
        );

        assert!(
            app.world().get::<ClassList>(plain).is_none(),
            "a non-focusable entity (no TabIndex) must not be tagged"
        );
        Ok(())
    }

    /// The selection resolves an asset path: a theme overlay when set, else the
    /// skin base.
    #[test]
    fn selection_resolves_the_entry_path() {
        let base = SkinSelection {
            skin: "graphite".to_owned(),
            theme: None,
        };
        assert_eq!(base.asset_path(), "skins/graphite/skin.css");
        let themed = SkinSelection {
            skin: "graphite".to_owned(),
            theme: Some("dark".to_owned()),
        };
        assert_eq!(themed.asset_path(), "skins/graphite/themes/dark.css");
    }

    /// Every shipped skin id has a base stylesheet on disk, and every declared
    /// theme overlay exists under its skin — so the switcher can never select a
    /// missing file.
    #[test]
    fn shipped_skins_and_themes_exist() -> Result<(), TestError> {
        for skin in SKINS {
            let base = skins_dir().join(skin).join("skin.css");
            assert!(base.is_file(), "missing skin base {}", base.display());
        }
        for (skin, theme) in THEMES {
            let overlay = skins_dir()
                .join(skin)
                .join("themes")
                .join(format!("{theme}.css"));
            assert!(overlay.is_file(), "missing theme {}", overlay.display());
        }
        Ok(())
    }
}
