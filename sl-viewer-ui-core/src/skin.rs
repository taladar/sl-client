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
//!   used as-is. The one paint that *does* have a side is a **bevel**, whose
//!   light source is physical and must not mirror; see
//!   [`BANNED_PHYSICAL_PROPERTIES`] for who is allowed to draw one.
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
//!   [`SkinRadius`] components. `resolve_skin_boxes` then folds those into the
//!   physical `Node` against the live [`UiDirection`], reusing the same
//!   [`LogicalRect::resolve`] mirror the scaffold's own boxes use — so a skin's
//!   `margin-inline-start` mirrors to the right edge under RTL for free.
//! - The **physical** originals (`margin-left`, `left`, `border-top-left-radius`,
//!   …) are **banned**: `scan_banned_properties` flags any of them, and the
//!   test suite fails the build if a shipped skin uses one. A skin author writes
//!   only the logical names.
//!
//! # i18n-aware skins
//!
//! The active locale is bridged onto the [`UiRoot`] as CSS **attributes**
//! (`dir="rtl"`, `lang="ja"`) by `sync_skin_attributes`, so a skin or overlay
//! can be locale-conditional with an attribute selector
//! (`:root[lang="ja"] { … }`) — and the culture-colour and colour-blind overlays
//! (`viewer-i18n-cultural-color-meanings` / `viewer-i18n-colorblind-accessibility`)
//! hook in the same way, through a `[data-culture]` / `[data-vision]` attribute.
//! Translated *strings* stay in Fluent; theme-authored localized labels/numbers
//! in CSS are a separate follow-up (`viewer-ui-skin-l10n-functions`), for which
//! the loader here leaves a preprocess seam.

use bevy::asset::embedded_asset;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::UiSystems;
// `CssPropertyRegistry`, `RegisterComponentPropertiesExt` and
// `ReflectStructPropertyRefExt` all arrive through the prelude
// (`pub use bevy_flair_core::*`).
use bevy_flair::prelude::*;
use bevy_flair::style::StyleSystems;
use sl_viewer_settings::env_pins::{EnvPinnedSettings, PinKind};

use crate::i18n::UiLocale;
use crate::skin_palette::SkinPalette;
use crate::ui::{LogicalRect, UiDirection, UiRoot, UiScaffoldSystems};

/// The asset-path directory (under the Bevy asset root) that holds the skins.
const SKINS_DIR: &str = "skins";

/// The file name of a skin's base stylesheet (no theme selected).
const SKIN_BASE_FILE: &str = "skin.css";

/// The subdirectory, under a skin, that holds its theme overlays.
const THEMES_DIR: &str = "themes";

/// The default skin id, used when neither the CLI nor the environment selects
/// one. Matches a directory under `SKINS_DIR`.
pub const DEFAULT_SKIN: &str = "graphite";

/// The skin ids that ship with the viewer, in switcher-cycle order. Each is a
/// directory under `assets/skins/` holding a `SKIN_BASE_FILE`.
pub const SKINS: &[&str] = &["graphite", "azure"];

/// The theme overlays that ship, keyed loosely by skin: a `(skin, theme)` pair
/// names `assets/skins/<skin>/themes/<theme>.css`. `None` in the switcher cycle
/// means "the skin's own base, no overlay".
pub const THEMES: &[(&str, &str)] = &[("graphite", "dark"), ("graphite", "relief")];

/// The asset path of the embedded fallback stylesheet — the sheet that is
/// always there, whatever the asset tree holds.
///
/// Widget state lives in the cascade rather than in a per-frame Rust paint
/// (`viewer-skin-widget-state-classes`), so a world whose skin assets did not
/// resolve would otherwise stop showing hovered / pressed / disabled at all.
/// Embedding one sheet in the binary removes that case, and nothing has to
/// branch on whether a skin loaded.
///
/// The crate segment is the **Rust** crate name (`module_path!()`, so
/// underscores), which is what [`embedded_asset!`] builds its path from.
pub const FALLBACK_STYLESHEET: &str = "embedded://sl_viewer_ui_core/skins/fallback.css";

/// Bake the fallback stylesheet and the structural rules it imports into the
/// binary.
///
/// Both files are embedded, not just the fallback: `common.css` is the single
/// copy of the structural rules, and a shipped skin reaches it by importing
/// [`FALLBACK_STYLESHEET`]. Embedding only the fallback would leave that
/// `@import` pointing at a file that exists in one asset source and not the
/// other — which resolves to nothing, silently, like every other link in this
/// chain.
pub fn embed_fallback_stylesheet(app: &mut App) {
    embedded_asset!(app, "skins/common.css");
    embedded_asset!(app, "skins/fallback.css");
}

/// The environment variable that seeds the initial [`SkinSelection`] skin id,
/// for the offline screenshot harness. The CLI `--skin` flag is the
/// user-facing selector; this is the debug-only override.
const SKIN_ENV: &str = "SL_VIEWER_SKIN";

/// The environment variable that seeds the initial theme overlay id.
const THEME_ENV: &str = "SL_VIEWER_THEME";

/// Record the `SL_VIEWER_SKIN` / `SL_VIEWER_THEME` seeds, if set, against the
/// Colors & Skins settings whose names the caller supplies (they live with the
/// preferences tab that binds them, not here).
///
/// [`Seed`](PinKind::Seed)s, not live pins: `apply_skin_setting` re-dresses the
/// UI from the stored pair the moment the user edits either combo, so the
/// controls work — they simply do not describe what this run started wearing.
pub fn record_env_pins(pins: &mut EnvPinnedSettings, skin_setting: &str, theme_setting: &str) {
    pins.pin_if_set(skin_setting, SKIN_ENV, PinKind::Seed);
    pins.pin_if_set(theme_setting, THEME_ENV, PinKind::Seed);
}

/// Which skin and (optional) theme overlay the UI is currently wearing.
///
/// Written by the CLI at start-up and by the gallery's switcher at runtime;
/// `apply_skin_selection` reloads the [`UiRoot`]'s stylesheet whenever it
/// changes, so a flip re-styles the whole tree live.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct SkinSelection {
    /// The skin id — a directory under `assets/skins/`.
    pub skin: String,
    /// The theme overlay id, or `None` for the skin's own base.
    pub theme: Option<String>,
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
    /// The initial selection: the CLI / `SKIN_ENV`+`THEME_ENV` values if
    /// **either** is given (the pair is taken atomically, so a `--skin` with
    /// no `--theme` means that skin's base — never a stored theme grafted onto
    /// a different skin), else the persisted preferences pair (the colors &
    /// skins tab, validated against the shipped [`SKINS`] / [`THEMES`] since a
    /// hand-edited file must not dress the UI in a missing stylesheet), else
    /// the default.
    #[must_use]
    pub fn resolve(
        skin: Option<String>,
        theme: Option<String>,
        stored_skin: Option<String>,
        stored_theme: Option<String>,
    ) -> Self {
        let cli_skin = skin.or_else(|| std::env::var(SKIN_ENV).ok());
        let cli_theme = theme.or_else(|| std::env::var(THEME_ENV).ok());
        if cli_skin.is_some() || cli_theme.is_some() {
            return Self {
                skin: cli_skin.unwrap_or_else(|| DEFAULT_SKIN.to_owned()),
                theme: cli_theme,
            };
        }
        let skin = stored_skin
            .filter(|stored| SKINS.contains(&stored.as_str()))
            .unwrap_or_else(|| DEFAULT_SKIN.to_owned());
        let theme = stored_theme.filter(|stored| {
            THEMES
                .iter()
                .any(|(theme_skin, theme)| *theme_skin == skin && *theme == *stored)
        });
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
    pub fn cycle_skin(&mut self) {
        let current = SKINS.iter().position(|candidate| *candidate == self.skin);
        let next_index = next_in_cycle(current, SKINS.len());
        if let Some(next) = SKINS.get(next_index) {
            (*next).clone_into(&mut self.skin);
        }
        self.theme = None;
    }

    /// Advance the theme overlay through the current skin's [`THEMES`] and back
    /// to `None` (the skin's own base). Drives the gallery's theme switcher.
    pub fn cycle_theme(&mut self) {
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
    #[must_use]
    pub fn label(&self) -> String {
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
pub struct ViewerSkinPlugin;

impl Plugin for ViewerSkinPlugin {
    fn build(&self, app: &mut App) {
        // The CSS engine. Brought up before our own property registration so its
        // `PropertyRegistry` / `CssPropertyRegistry` resources exist to extend.
        app.add_plugins(FlairPlugin);
        embed_fallback_stylesheet(app);
        register_logical_properties(app);
        register_caret_properties(app);
        register_chat_band_properties(app);
        // The chrome role palette every Rust-painted widget state reads
        // (`viewer-audit-skin-token-coverage`).
        crate::skin_palette::register_palette_properties(app);
        // The `-sk-uisnd-<key>` UI-sound overrides are registered by
        // `sl-viewer-ui-sounds`' own plugin, which is why that one is added
        // after this one: `bevy_flair` has to be up before its registries can be
        // extended, and it only snapshots them into the CSS loader in `finish`.
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
                    // Give everything a `:hover` rule selects the component
                    // that pseudo-class is read from — nothing else does
                    // (`viewer-skin-list-row-striping`).
                    stamp_hover_state,
                    // And everything an `:active` rule selects the press
                    // state that pseudo-class is read from
                    // (`viewer-skin-active-class-means-selected-not-pressed`).
                    stamp_press_state,
                ),
            )
            .add_observer(press_tracked_on_press)
            .add_observer(press_tracked_on_release)
            .add_observer(press_tracked_on_drag_end)
            .add_observer(press_tracked_on_cancel)
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

/// The CSS class on ordinary body text — a caption, a row label, a button's
/// own glyph.
///
/// It carries the resting colour (`--text-primary`), which is the whole point:
/// every state rule that greys or highlights text is a *descendant* or
/// *compound* selector over a node like this, and a label with no base class
/// has nowhere to fall back to when the state lifts. Named here because eight
/// crates were each declaring their own copy of the same string.
pub const TEXT_CLASS: &str = "sk-text";

/// The CSS class on the row a drag is currently over — the folder an
/// inventory drop would land in.
///
/// Louder than [`SELECTED_CLASS`] and written to beat it, because during a drag
/// the question on screen is "where will this land", not "what was selected
/// before it started". It used to be arranged by *skipping* the selection
/// system for the duration of the drag; the cascade settles it instead.
pub const DROP_TARGET_CLASS: &str = "sk-drop-target";

/// The CSS classes on an RLVa console line, by what the line *is*: a reply the
/// grid sent back, an accepted command, a refused one. A typed command carries
/// none of them and reads as plain text.
///
/// Meaning-bearing, so tokens rather than constants — for the same reason the
/// four notification kinds are: "was that refused?" must survive a skin, and a
/// colour-blind overlay is exactly what retunes it. Compound with
/// [`TEXT_CLASS`], which the line also carries, so dropping one falls back to
/// the plain look.
pub const CONSOLE_REPLY_CLASS: &str = "sk-console-reply";

/// The accepted-command half of [`CONSOLE_REPLY_CLASS`].
pub const CONSOLE_INFO_CLASS: &str = "sk-console-info";

/// The refused-command half of [`CONSOLE_REPLY_CLASS`].
pub const CONSOLE_ERROR_CLASS: &str = "sk-console-error";

/// The CSS class on a line that reports a **failure** — a refused save, a
/// compile error. Compound with [`TEXT_CLASS`]; see [`text_meaning`].
pub const ERROR_TEXT_CLASS: &str = "sk-error";

/// The CSS class on a line that **warns** without having failed yet — a
/// teleport taking longer than it should.
pub const WARN_TEXT_CLASS: &str = "sk-warn";

/// The CSS class on a **note**: a line shown exactly when something wants the
/// reader's eye, like the count of settings a filter matched.
pub const NOTE_TEXT_CLASS: &str = "sk-note";

/// The CSS class on text in the **experience** family's accent, so every
/// experience surface reads as one thing.
pub const EXPERIENCE_TEXT_CLASS: &str = "sk-experience";

/// The CSS class on an **overlay** drawn over the rendered world rather than
/// inside a panel: a beacon's label, a diagnostic read-out. Its backdrop is
/// `--overlay-bg`, the scrim a docked floater already uses.
pub const OVERLAY_CLASS: &str = "sk-overlay";

/// The CSS class on an overlay's **text**.
///
/// Its own token rather than a text role, and not because a skin should not
/// choose it — it should: the difference is what the text is read *against*.
/// Panel roles are tuned against panel surfaces, while this is read against
/// whatever the camera happens to be pointing at, so the skin needs to answer
/// the two questions separately.
pub const OVERLAY_TEXT_CLASS: &str = "sk-overlay-text";

/// The CSS class on a **tooltip**'s box (`viewer-skin-tooltip-roles`): its
/// face, frame, corners, padding and wrap width, all from the `--tooltip-*`
/// tokens.
///
/// One class for every tip — the world hover tip, a link's URL tip, the
/// minimap's and the world map's — because the reference treats the tooltip
/// as one skinned widget (`tool_tip.xml`), and a skin whose floaters are dark
/// grey draws its tips as a *light* plate precisely so that "the viewer is
/// telling you something" does not read as part of the panel under it. Four
/// hand-spelled looks could never agree on that.
///
/// Spawn it through [`tooltip_box`], which also carries the two properties a
/// skin is not allowed to change.
pub const TOOLTIP_CLASS: &str = "sk-tooltip";

/// The CSS class on a tooltip's **text** (`--tooltip-text`). A rule of its own
/// rather than a `color` on [`TOOLTIP_CLASS`], because `bevy_ui` has no style
/// inheritance: the box's colour would never reach the text node inside it.
pub const TOOLTIP_TEXT_CLASS: &str = "sk-tooltip-text";

/// The CSS class on an **inspector** card — the small click-to-open popup
/// about an avatar or an object (`--inspector-bg` / `--inspector-border`).
///
/// Not a tooltip, although it pops up where a tooltip would: the reference
/// builds it as a floater with its own plate (`Inspector_Background`), it
/// takes clicks where a tip must never take one, and a skin that draws its
/// tips as a light plate keeps its inspector dark. Its text and buttons are
/// already skinned through their own roles; this is the card they sit on.
pub const INSPECTOR_CLASS: &str = "sk-inspector";

/// A tooltip's face before the stylesheet lands, and in a headless world that
/// resolves none: `fallback.css`'s `--tooltip-bg`. The skin decides the real
/// one; `tooltip_fallback_matches_the_fallback_sheet` holds the two together.
///
/// Opaque on purpose: a tip is read over whatever is under the pointer — chat
/// text, a field, a map — and any translucency lets that bleed into the words.
pub const TOOLTIP_BACKGROUND: Color = Color::srgb(0.06, 0.07, 0.10);

/// A tooltip's frame colour before the stylesheet lands (`--tooltip-border`).
pub const TOOLTIP_BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// A tooltip's text colour before the stylesheet lands (`--tooltip-text`).
pub const TOOLTIP_TEXT: Color = Color::srgb(0.95, 0.95, 0.95);

/// A tooltip's frame width before the stylesheet lands, in logical pixels
/// (`--tooltip-border-width`).
pub const TOOLTIP_BORDER_WIDTH: f32 = 1.0;

/// A tooltip's corner radius before the stylesheet lands, in logical pixels
/// (`--tooltip-radius`).
pub const TOOLTIP_RADIUS: f32 = 4.0;

/// A tooltip's padding above and below its text before the stylesheet lands,
/// in logical pixels (`--tooltip-padding-block`).
pub const TOOLTIP_PADDING_BLOCK: f32 = 5.0;

/// A tooltip's padding either side of its text before the stylesheet lands, in
/// logical pixels (`--tooltip-padding-inline`).
pub const TOOLTIP_PADDING_INLINE: f32 = 8.0;

/// The width a tooltip wraps at before the stylesheet lands, in logical pixels
/// (`--tooltip-max-width`).
pub const TOOLTIP_MAX_WIDTH: f32 = 400.0;

/// An inspector card's face before the stylesheet lands (`--inspector-bg`).
pub const INSPECTOR_BACKGROUND: Color = Color::srgba(0.08, 0.09, 0.13, 0.98);

/// An inspector card's frame before the stylesheet lands
/// (`--inspector-border`).
pub const INSPECTOR_BORDER: Color = Color::srgb(0.32, 0.36, 0.44);

/// The global z-order every tooltip draws at: above every other UI layer,
/// the inspector card included, so a tip can describe anything on screen.
pub const TOOLTIP_Z: i32 = i32::MAX;

/// A tooltip's box, hidden-agnostic and unpositioned: an absolutely placed
/// column wearing [`TOOLTIP_CLASS`], with the fallback look beside the class
/// for the frame before the stylesheet lands.
///
/// The caller adds its own `Visibility` (or `Display`), `Name` and parent, and
/// writes `left` / `top` as the pointer moves. What it does **not** get to
/// choose, and neither does a skin, is the pair that makes a tip a tip:
///
/// - [`Pickable::IGNORE`] — a tip must never swallow the click or the hover
///   meant for the thing it describes, which is directly under it;
/// - [`GlobalZIndex`]`(`[`TOOLTIP_Z`]`)` — a tip that falls behind a floater
///   describes nothing.
///
/// Neither is a style property, so no stylesheet can reach them; that is the
/// point of spawning them here rather than leaving each consumer to remember.
#[must_use]
pub fn tooltip_box() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            max_width: Val::Px(TOOLTIP_MAX_WIDTH),
            padding: UiRect::axes(
                Val::Px(TOOLTIP_PADDING_INLINE),
                Val::Px(TOOLTIP_PADDING_BLOCK),
            ),
            border: UiRect::all(Val::Px(TOOLTIP_BORDER_WIDTH)),
            border_radius: BorderRadius::all(Val::Px(TOOLTIP_RADIUS)),
            ..default()
        },
        BackgroundColor(TOOLTIP_BACKGROUND),
        BorderColor::all(TOOLTIP_BORDER),
        ClassList::new_with_classes([TOOLTIP_CLASS]),
        Pickable::IGNORE,
        GlobalZIndex(TOOLTIP_Z),
    )
}

/// A tooltip's text colour and class — [`TOOLTIP_TEXT_CLASS`] beside the
/// fallback [`TOOLTIP_TEXT`] — for the text node inside a [`tooltip_box`].
/// The text node is the caller's, since only it knows its font and content;
/// it should carry [`Pickable::IGNORE`] too.
#[must_use]
pub fn tooltip_text() -> (TextColor, ClassList) {
    (
        TextColor(TOOLTIP_TEXT),
        ClassList::new_with_classes([TOOLTIP_TEXT_CLASS]),
    )
}

/// The CSS class on an inventory **folder**'s label, which the reference draws
/// gold against an item's plain text.
///
/// A role rather than drift: the distinction is what tells a container from a
/// thing at a glance down a long tree, and a high-contrast or colour-blind
/// skin wants to retune it — the same argument as [`PRESENCE_ONLINE_CLASS`].
pub const FOLDER_LABEL_CLASS: &str = "sk-folder-label";

/// The CSS class on one tile of a dense grid — an emoji cell, a tone swatch.
/// Its hover is a `:hover` rule and needs no code; the class exists so the rule
/// has something to select.
pub const TILE_CLASS: &str = "sk-tile";

/// The CSS class on a clickable item embedded in notecard prose — an object in
/// a paragraph. Its hover is a `:hover` rule, so the class is what the
/// scaffold's hover stamp needs to see.
pub const INLINE_ITEM_CLASS: &str = "sk-inline-item";

/// The CSS class on one row of a combo box's open drop-down. Its hover is a
/// `:hover` rule, which replaced a hand-written `Pointer<Over>` /
/// `Pointer<Out>` observer pair — and then painted nothing until the
/// scaffold's hover stamp gave the row the state to be hovered in.
pub const COMBO_OPTION_CLASS: &str = "sk-combo-option";

/// The CSS class on a presence indicator showing its subject is online, and
/// [`PRESENCE_OFFLINE_CLASS`] for one who is not.
///
/// Meaning-bearing, like `--gain` / `--loss`: green-for-present is a
/// convention, not a fact, and a colour-blind or high-contrast skin wants to
/// remap it — which is exactly why these are roles rather than two constants in
/// the Friends list. The *glyph* changes too (a filled dot against a hollow
/// one), so the state still reads with the colours taken away.
pub const PRESENCE_ONLINE_CLASS: &str = "sk-presence-online";

/// The offline half of [`PRESENCE_ONLINE_CLASS`]. One of the pair is always on:
/// there is no third presence and so no resting colour to fall back to.
pub const PRESENCE_OFFLINE_CLASS: &str = "sk-presence-offline";

/// The CSS class on a secondary line — an instruction under a heading, the
/// muted half of a label/value pair. [`TEXT_CLASS`]'s quieter sibling.
pub const TITLE_CLASS: &str = "sk-title";

/// The CSS class on the one line that names a section inside a panel.
pub const HEADING_CLASS: &str = "sk-heading";

/// The CSS class on a **push button** — the bordered box
/// [`ButtonSpec::bordered`](crate::ui_spawn::ButtonSpec::bordered) spawns.
///
/// It carries the whole resting look: the control shade, a 2 px border, the
/// control radius and its own padding, so a skin can restate the *shape* of
/// every button in the viewer and not merely its tint
/// (`viewer-skin-image-backed-widgets` swaps the background for a nine-slice
/// from here).
///
/// Named here because **seventeen files had declared their own copy of the
/// string** — and, more to the point, because the class is now a *default*
/// rather than something a call site asks for. It used to be an `Option` that
/// began at `None`: a button was skinned only where its panel remembered to
/// chain `.class(…)`, which 14 of the 30 `ButtonSpec` sites (and every one of
/// the 33 hand-rolled boxes) did not.
pub const BUTTON_CLASS: &str = "sk-button";

/// [`BUTTON_CLASS`] at **row scale**: the same button where the standard box
/// does not belong — inside a list row, a table cell, or a dense control strip
/// beside a field.
///
/// A modifier rather than a class of its own, so it is worn *with*
/// [`BUTTON_CLASS`] and restates only the geometry. It exists because the
/// hand-rolled boxes it replaced were not all one size: a panel's action row
/// was padded 10×5 while a cell's "remove" button was padded 6×1, and giving
/// the second the first's box would push every row of an access list eight
/// pixels further apart. Which of the two a button is remains a decision at the
/// call site; what each *looks* like is the skin's.
pub const COMPACT_BUTTON_CLASS: &str = "sk-button-compact";

/// [`BUTTON_CLASS`] as the **call to act**: the one button in its row the
/// moment is asking for — Retry on a failed teleport, Stand Up while seated.
///
/// A modifier like [`COMPACT_BUTTON_CLASS`], but restating the *fill*
/// (`--button-primary-bg`) rather than the geometry, so the two combine. It
/// exists because those buttons used to say it with an inline blue, which
/// `.sk-button` then painted over: once the class became a default, the only
/// thing that had set them apart was a colour no stylesheet could see.
pub const PRIMARY_BUTTON_CLASS: &str = "sk-button-primary";

/// The CSS class on a flat action button — the shape a panel's button column
/// spawns ([`ButtonSpec::flat`](crate::ui_spawn::ButtonSpec::flat)).
///
/// Deliberately *not* [`BUTTON_CLASS`]: that class carries a whole resting look
/// (the control shade, a 2 px border, its own padding) which these buttons do
/// not have and should not suddenly grow. This one carries only the refused
/// state, so the resting look stays the panel's until
/// `viewer-skin-panel-text-roles` moves it to tokens as well.
pub const ACTION_BUTTON_CLASS: &str = "sk-action-button";

/// The CSS class on a bottom-toolbar button. Named here rather than beside the
/// toolbar because `PRESS_CLASSES` has to name it: the button is a plain
/// pickable box, and its `:active` rule matches only what
/// `stamp_press_state` gives it.
pub const TOOLBAR_BUTTON_CLASS: &str = "sk-toolbar-button";

/// The CSS class on a floater's title-bar glyph button (close, minimise,
/// dock) — `--glyph-button-bg`. Here for the same reason as
/// [`TOOLBAR_BUTTON_CLASS`].
pub const FLOATER_BUTTON_CLASS: &str = "sk-floater-button";

/// The CSS class on a control whose action does not apply right now — greyed
/// rather than removed, so a row of actions keeps its shape as the selection
/// moves. [`DISABLED_TEXT_CLASS`] greys its label.
///
/// Named here rather than in the one panel that first needed it: it is the
/// oldest member of the state vocabulary below, and the pair proved the shape
/// the rest follow.
pub const DISABLED_SURFACE_CLASS: &str = "sk-disabled-surface";

/// The label half of [`DISABLED_SURFACE_CLASS`].
pub const DISABLED_TEXT_CLASS: &str = "sk-disabled-text";

/// The CSS class on one row of a list a panel builds itself, rather than
/// through the table widget — the pickers, the inventory tree, the About box's
/// licence list. Its selected look is [`SELECTED_CLASS`]; this carries the
/// resting one, so dropping the state class has somewhere to land.
///
/// The table widget's own rows use [`TABLE_ROW_CLASS`], which shares every
/// rule with this one.
pub const LIST_ROW_CLASS: &str = "sk-list-row";

/// The CSS class on one row of the **table widget**, which shares every rule
/// with [`LIST_ROW_CLASS`] — "table row" would be a lie on half the lists that
/// carry the other one, and a row's look must not depend on which of the two
/// spelled it.
///
/// A marker rather than a pseudo-class, and deliberately: the rows are
/// **recycled** by the virtual list, so selection is a property of the row's
/// current *index* rather than of the entity, and no state the engine tracks
/// describes it.
pub const TABLE_ROW_CLASS: &str = "sk-table-row";

/// Every class `common.css` writes a `:hover` rule for.
///
/// It is the input to [`stamp_hover_state`] and
/// `every_hover_rule_has_something_to_hover` asserts it is **exactly** the set
/// the stylesheet names — add a `:hover` rule without adding its class here and
/// the test says so, rather than the rule quietly painting nothing.
const HOVER_CLASSES: &[&str] = &[
    BUTTON_CLASS,
    // Always worn with `BUTTON_CLASS`, so never the only reason a node is
    // stamped — but a class with a `:hover` rule of its own is named here, or
    // the list stops being the one place that says which rules can fire.
    PRIMARY_BUTTON_CLASS,
    TILE_CLASS,
    INLINE_ITEM_CLASS,
    COMBO_OPTION_CLASS,
    TABLE_ROW_CLASS,
    LIST_ROW_CLASS,
    SCROLLBAR_THUMB_CLASS,
    SCROLLBAR_ARROW_CLASS,
    TAB_SCROLL_BUTTON_CLASS,
];

/// What [`stamp_hover_state`] walks: the classed nodes that could still need
/// the hover component — everything whose `ClassList` has just changed (an
/// insert counts) and that carries neither the modern `Hovered` nor the legacy
/// `Interaction`.
///
/// A named type because the two `Without`s and the `Changed` are what keep the
/// scan bounded, and a filter this load-bearing should be readable at a glance
/// rather than spelled inline.
type HoverCandidates<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static ClassList),
    (
        Changed<ClassList>,
        Without<bevy::picking::hover::Hovered>,
        Without<Interaction>,
    ),
>;

/// Give everything a `:hover` rule selects the component that pseudo-class is
/// read from, so the rule fires at all.
///
/// `bevy_picking`'s `Hovered` is **opt-in** — its own docs say "typically, a
/// simple hoverable entity or widget will have this component added to it", and
/// nothing adds it for you — and `bevy_flair` reads exactly two things:
/// `Hovered`, and the legacy `Interaction` that **`bevy_ui`'s** `Button`
/// requires. Neither is implied by being pickable, or by carrying a class, or
/// by `bevy_ui_widgets`' headless `Button`, which is a different type that
/// requires neither.
///
/// So a `:hover` rule on anything that is not a `bevy_ui::Button` parsed,
/// matched nothing, and painted nothing — silently, and only in the live
/// viewer. Three rules were in that state when this was written: the scroll
/// list rows this was added for, the emoji grid's `.sk-tile`, and
/// `.sk-combo-option`, the last two having had working `Pointer<Over>` /
/// `Pointer<Out>` observer pairs *replaced* by the rule.
///
/// A cascade test cannot catch it, which is worth stating because this codebase
/// leans on those: a test inserts `Hovered(true)` itself and so proves the
/// *rule* while staying blind to whether anything ever supplies the state.
/// `every_hover_rule_has_something_to_hover` is the check that does see it.
///
/// Keyed off the classes rather than wired per call site, on
/// [`stamp_focus_ring_class`]'s model: one place, and a widget built next year
/// gets its hover by carrying the class, which is the whole claim the class
/// makes. `Without<Hovered>` keeps the scan to nodes that still need it — the
/// striping and selection systems touch a row's `ClassList` often — and
/// `Without<Interaction>` leaves anything that already has the legacy component
/// to the path it had, rather than have two systems write one pseudo-state.
fn stamp_hover_state(mut commands: Commands, hoverable: HoverCandidates) {
    for (entity, classes) in &hoverable {
        if HOVER_CLASSES.iter().any(|class| classes.contains(*class)) {
            commands
                .entity(entity)
                .insert(bevy::picking::hover::Hovered::default());
        }
    }
}

/// Every class `common.css` writes an `:active` rule for — every button family
/// the viewer has.
///
/// The input to [`stamp_press_state`]; `every_press_rule_has_something_to_press`
/// asserts it is exactly the set the stylesheet names, for the reason
/// [`HOVER_CLASSES`] has the same test.
const PRESS_CLASSES: &[&str] = &[
    BUTTON_CLASS,
    ACTION_BUTTON_CLASS,
    TOOLBAR_BUTTON_CLASS,
    FLOATER_BUTTON_CLASS,
    SCROLLBAR_ARROW_CLASS,
    TAB_SCROLL_BUTTON_CLASS,
];

/// A button box whose press the skin tracks itself: [`stamp_press_state`] puts
/// it on a node that wears a [`PRESS_CLASSES`] class and carries no button
/// component, and the pointer observers keep `bevy_ui::Pressed` on it while the
/// primary button holds it down.
#[derive(Component, Debug, Clone, Copy, Default)]
struct PressTracked;

/// What [`stamp_press_state`] walks: classed nodes that could still need their
/// press tracked. Both button components are excluded because each already
/// supplies the pseudo-state — `bevy_ui`'s through `Interaction`,
/// `bevy_ui_widgets`' by keeping `Pressed` itself — and two writers of one
/// state is a race.
type PressCandidates<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static ClassList),
    (
        Changed<ClassList>,
        Without<PressTracked>,
        Without<Interaction>,
        Without<bevy::ui_widgets::Button>,
    ),
>;

/// Give every button box an `:active` rule selects a press state the rule can
/// read (`viewer-skin-active-class-means-selected-not-pressed`).
///
/// `bevy_flair`'s `:active` is `Interaction::Pressed` or the `Pressed` marker,
/// and a button spawned as a plain pickable box — every flat action button, the
/// bottom toolbar, a floater's close box — has neither. So the pressed rule
/// parsed, matched nothing and painted nothing, which is why a press was
/// invisible on every one of them. Keyed off the classes, on
/// [`stamp_hover_state`]'s model: a button built next year gets its press by
/// wearing the class.
fn stamp_press_state(mut commands: Commands, candidates: PressCandidates) {
    for (entity, classes) in &candidates {
        if PRESS_CLASSES.iter().any(|class| classes.contains(*class)) {
            commands.entity(entity).insert(PressTracked);
        }
    }
}

/// A primary press on a [`PressTracked`] box holds it down — unless it is
/// refused, which a press must never appear to reach.
///
/// Never stops the propagation: the box's own `Pointer<Press>` observer, and any
/// ancestor's, still see the press exactly as before.
fn press_tracked_on_press(
    press: On<Pointer<Press>>,
    tracked: Query<Has<bevy::ui::InteractionDisabled>, With<PressTracked>>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(disabled) = tracked.get(press.entity)
        && !disabled
    {
        commands.entity(press.entity).insert(bevy::ui::Pressed);
    }
}

/// Let a [`PressTracked`] box up again. The three ways a hold ends are the
/// three `bevy_ui_widgets`' button listens for: the release over it, the end
/// of a drag that carried the pointer off it, and a cancel.
fn release_tracked(
    entity: Entity,
    tracked: &Query<(), With<PressTracked>>,
    commands: &mut Commands,
) {
    if tracked.contains(entity) {
        commands.entity(entity).remove::<bevy::ui::Pressed>();
    }
}

/// [`release_tracked`] on a release over the box.
fn press_tracked_on_release(
    release: On<Pointer<Release>>,
    tracked: Query<(), With<PressTracked>>,
    mut commands: Commands,
) {
    if release.button == PointerButton::Primary {
        release_tracked(release.entity, &tracked, &mut commands);
    }
}

/// [`release_tracked`] at the end of a drag that began on the box.
fn press_tracked_on_drag_end(
    drag_end: On<Pointer<DragEnd>>,
    tracked: Query<(), With<PressTracked>>,
    mut commands: Commands,
) {
    if drag_end.button == PointerButton::Primary {
        release_tracked(drag_end.entity, &tracked, &mut commands);
    }
}

/// [`release_tracked`] when the pointer is cancelled.
fn press_tracked_on_cancel(
    cancel: On<Pointer<Cancel>>,
    tracked: Query<(), With<PressTracked>>,
    mut commands: Commands,
) {
    release_tracked(cancel.entity, &tracked, &mut commands);
}

/// The CSS class on every other row of a scroll list
/// (`viewer-skin-list-row-striping`), stamped by
/// [`stripe_virtual_rows`](crate::virtual_list::stripe_virtual_rows) from the
/// row's **data** index rather than from its pool slot.
///
/// Striping is not decoration on a list a hundred rows long — it is how the eye
/// keeps a row's cells together across a wide table, which is most of what an
/// inventory list, a radar and a region's top-objects list are.
///
/// A class rather than the `:nth-child(even)` the engine does parse: the rows
/// are **recycled**, so a row's position among its siblings is its pool slot
/// and the slot↔item mapping is modular — a `:nth-child` stripe would walk up
/// the list as it scrolled, which is worse than no stripe. Only ever written
/// compound with a row class in `common.css`, so a pooled row belonging to
/// something that is not a list is unaffected.
pub const STRIPE_CLASS: &str = "sk-stripe";

/// The CSS class on the **face** a scroll list's rows sit on — the viewport of
/// a virtualised list (`viewer-skin-light-surface-roles`).
///
/// A *data* surface rather than chrome, which is the distinction the whole
/// field family exists to make: the reference's classic skins put a light sage
/// list inside a dark grey window, and one surface role cannot be both. The
/// dark skins give `--list-bg` the scrim twenty-two panels each declared as
/// their own `const LIST_BACKGROUND`, so nothing moved when they stopped.
///
/// It also **re-roots the text roles** inside it: `common.css` resolves
/// [`TEXT_CLASS`] and [`TITLE_CLASS`] under this class to the field family, so
/// a skin that makes the face light gets black rows with no Rust involved. A
/// button dropped into a row brings its own chrome back with it.
///
/// The table widget puts it on its own viewport, so every table has it without
/// asking; a panel that builds a virtualised list by hand adds it there.
pub const LIST_SURFACE_CLASS: &str = "sk-list-surface";

/// The CSS class on a field that may be read and copied but not changed —
/// `ui_text_input`'s `ReadOnlyField`, mirrored by `reflect_read_only_field`.
///
/// A class rather than a pseudo-class because **no pseudo-class can see it**:
/// a read-only field still takes focus and still shows a caret (that is what
/// `Ctrl+C` needs), so it is neither `:disabled` nor ordinary. Its own role
/// rather than the greyed one, because the reference sends a read-only editor
/// back to chrome grey with light text — the surface says "not here", not a
/// dimmed glyph colour.
pub const READ_ONLY_CLASS: &str = "sk-read-only";

/// The CSS class on a field's prompt — the search box's placeholder and its
/// leading glyph, shown while the field is empty.
///
/// `TextFgTentativeColor` in the reference: its own role rather than the muted
/// chrome text, because it is read against the *field's* face and goes away
/// the moment anything is typed.
pub const FIELD_PLACEHOLDER_CLASS: &str = "sk-field-placeholder";

/// The CSS class on a **container whose descendant has keyboard focus** — CSS's
/// `:focus-within`, which `bevy_flair` does not have.
///
/// The engine parses six non-tree-structural pseudo-classes and this is not
/// among them; `:has()` parses but would not *invalidate*, since a descendant's
/// focus moving marks only that descendant's style data (`sync_input_focus`),
/// leaving an ancestor rule right in a unit test and intermittent on screen. So
/// the widget that knows which editor is inside it mirrors the state onto its
/// own box, the way every state the engine cannot see is mirrored.
///
/// Its one wearer today is the search box (`ui_search.rs`), which is a
/// container around a *bare* field: `.sk-field:focus` reaches a decorated field
/// because the class and the editor are one entity, and the search box is
/// exactly the case where they are not.
pub const FOCUS_WITHIN_CLASS: &str = "sk-focus-within";

/// The CSS class on a row the pointer or the keyboard has lit
/// (`--control-bg-hover`), and [`HIGHLIGHTED_TEXT_CLASS`] for its label.
///
/// Not `:hover`: the engine's pseudo-class sees only the pointer, and a menu
/// row is equally lit by keyboard navigation and by being the ancestor of an
/// open sub-menu. Where a state genuinely *is* a pointer hover, write the
/// `:hover` rule instead and skip the system entirely.
pub const HIGHLIGHTED_CLASS: &str = "sk-highlighted";

/// The label half of [`HIGHLIGHTED_CLASS`]. Two classes because `bevy_ui` has
/// no style inheritance — a row and its text are separate nodes.
pub const HIGHLIGHTED_TEXT_CLASS: &str = "sk-highlighted-text";

/// The CSS class on text that matched an active filter term — a preferences
/// search, a menu search. The glyphs take the skin's `--match-highlight`, and
/// the row keeps whatever colour it had.
pub const MATCH_CLASS: &str = "sk-match";

/// The CSS class on text a live filter term left with nothing — a preferences
/// tab whose every row the search collapsed. The counterpart to
/// [`MATCH_CLASS`]: one says "this is the hit", the other "there are none
/// here". Absent when no filter is running, so the resting colour is the
/// widget's own.
pub const NO_MATCH_CLASS: &str = "sk-no-match";

/// The CSS class on the **selected** item of a collection — a row of a scroll
/// list or table, a gallery tile, a keyframe marker on a timeline.
///
/// Named for what it means, and on purpose not `sk-active`, which it used to
/// be: CSS's `:active` is the pointer holding a control down, and a skin author
/// who knows CSS read `.sk-active` as the pressed state and wrote the wrong
/// rule. A class because the rows are recycled by the virtual list, so which one
/// is selected is a property of its *data* index, which no engine state tracks.
///
/// A **toggle** is not selected, it is on — a toolbar button whose floater is
/// open, a time-of-day preset in force. Those carry `bevy_ui::Checked`, the
/// engine's own `:checked`, like a tab or a tick box. The press itself is
/// `:active`, supplied by `stamp_press_state` wherever a button component
/// does not already supply it.
pub const SELECTED_CLASS: &str = "sk-selected";

/// The CSS class on the title band of the **front-most** floater — the window
/// keyboard and `Ctrl+W` act on — and [`FRONTMOST_TEXT_CLASS`] on its title.
///
/// Not "active" and not "focused", for the same reason as [`SELECTED_CLASS`]:
/// both are CSS pseudo-classes (`:active`, `:focus`) meaning something else,
/// and the front-most window need not hold keyboard focus at all.
pub const FRONTMOST_CLASS: &str = "sk-frontmost";

/// The label half of [`FRONTMOST_CLASS`].
pub const FRONTMOST_TEXT_CLASS: &str = "sk-frontmost-text";

/// The CSS class on text drawn in the skin's **accent** — a mark that says
/// "this one" without being a selection: the active group's name and marker,
/// a list's sort arrow, a profile's group links.
///
/// It used to be the text half of the selection class, which is how a label
/// that was never selected came to wear the selection's name.
pub const ACCENT_TEXT_CLASS: &str = "sk-accent-text";

/// The CSS class on a widget asking to be noticed (unread IMs behind a closed
/// Conversations window). The viewer says only *that* it wants attention; the
/// skin decides whether that pulses, glows or simply stays lit.
pub const ATTENTION_CLASS: &str = "sk-attention";

/// The skin class a colour *names*, or `None` for one that is not a text role.
///
/// The seam `viewer-skin-panel-text-roles` turns on. Panels take their label
/// colours from [`SkinPalette::FALLBACK`](crate::skin_palette::SkinPalette::FALLBACK) — `viewer-audit-skin-token-coverage`
/// collapsed all 64 copies onto those four constants, so a panel asking for
/// `text_muted` is *naming a role*, not picking a grey, and the equality below
/// is exact rather than approximate.
///
/// Reading the role back out of the value is what lets every call site stay as
/// it is. The alternative — a role parameter on each helper — is the same
/// information spelled at ~230 call sites.
///
/// A colour that matches no role gets no class and keeps painting itself. That
/// is deliberate: a panel with a colour of its own has not said which role it
/// means, and guessing would be worse than leaving it unskinned for
/// [`crate::skin`]'s later passes.
///
/// # The invariant this rests on
///
/// Reading a role out of a value only works while the mapping is **injective**:
/// each of the four values must belong to exactly one role in the whole
/// palette, or a label would silently take another role's class.
/// `the_text_roles_are_the_only_roles_with_their_values` asserts it over every
/// field of [`SkinPalette::FALLBACK`](crate::skin_palette::SkinPalette::FALLBACK) by reflection, so a future palette edit
/// that collides two roles fails there rather than quietly giving labels the
/// wrong class.
///
/// Only the **fallback** has to satisfy it. A *skin* may give two roles the
/// same value freely: the class is chosen here, from the fallback, and the skin
/// then paints whatever it likes per class. And a caller who needs a role this
/// cannot express passes the class explicitly — `spawn_text`'s own argument, or
/// [`ButtonSpec::label_class`](crate::ui_spawn::ButtonSpec::label_class) — which always wins over the derivation.
#[must_use]
pub const fn role_class(color: Color) -> Option<&'static str> {
    // `match` cannot pattern-match on non-structural constants, so this is a
    // chain of comparisons against the four role values.
    if color_eq(color, SkinPalette::FALLBACK.text_primary) {
        Some(TEXT_CLASS)
    } else if color_eq(color, SkinPalette::FALLBACK.text_muted) {
        Some(TITLE_CLASS)
    } else if color_eq(color, SkinPalette::FALLBACK.text_heading) {
        Some(HEADING_CLASS)
    } else if color_eq(color, SkinPalette::FALLBACK.text_disabled) {
        Some(DISABLED_TEXT_CLASS)
    } else {
        None
    }
}

/// `Color` equality usable from a `const fn`: the roles are all `Srgba`, and
/// both sides are compile-time constants copied from the same place, so exact
/// component equality is the right test and not a float-tolerance question.
const fn color_eq(left: Color, right: Color) -> bool {
    match (left, right) {
        (Color::Srgba(left), Color::Srgba(right)) => {
            left.red == right.red
                && left.green == right.green
                && left.blue == right.blue
                && left.alpha == right.alpha
        }
        _ => false,
    }
}

/// A [`TextColor`] and the class its role names, as a spawn bundle.
///
/// The form [`role_class`] takes at a **spawn site**: `text_role(LABEL_COLOR)`
/// in place of `TextColor(LABEL_COLOR)` is the whole edit, so a panel that
/// builds its own text nodes rather than going through
/// [`crate::ui_spawn`]'s helpers becomes skinnable without restating which
/// role it meant — the colour already says.
///
/// A colour that names no role yields an **empty** `ClassList` rather than
/// none: the node is then ready for a class to be written into it later (a
/// table cell's role changes per bind), and a widget that wanted one would
/// otherwise find nothing there.
///
/// Only for text whose colour is **not** rewritten from Rust afterwards. A
/// class `color` beats a `TextColor`, so putting one on a node that something
/// still repaints per state or per frame — `chat.rs` fades a line by its age —
/// would pin it to one colour. Those are the sites that need a state class, or
/// to stay as they are.
#[must_use]
pub fn text_role(color: Color) -> (TextColor, ClassList) {
    (
        TextColor(color),
        role_class(color).map_or_else(ClassList::empty, |class| {
            ClassList::new_with_classes([class])
        }),
    )
}

/// A [`TextColor`] and a **meaning** class, as a spawn bundle.
///
/// The escape hatch [`text_role`] deliberately does not provide: a colour that
/// names none of the four roles because it means something the roles cannot say
/// — a warning, a failure, a note asking for the eye, the emerald every
/// experience surface wears. Those are not drift to be collapsed; the
/// distinction has to survive a skin, and a colour-blind overlay is exactly
/// what retunes it. So each gets a token and a class of its own, compounded
/// over [`TEXT_CLASS`] the way an RLVa console line's is — `.sk-text.sk-warn`
/// beats `.sk-text` on specificity, wherever the two sit in the sheet.
///
/// The colour passed stays the skinless fallback, as everywhere else.
#[must_use]
pub fn text_meaning(color: Color, meaning: &'static str) -> (TextColor, ClassList) {
    (
        TextColor(color),
        ClassList::new_with_classes([TEXT_CLASS, meaning]),
    )
}

/// Put the role class `color` names on `list`, taking off whichever of the
/// other three was there.
///
/// [`role_class`] answers "which role is this"; this keeps a node's answer
/// current when the colour it is painted with **changes** — a table cell whose
/// row went from present to muted, say. Exactly one of the four is ever on, and
/// a colour that names no role leaves the node with none of them.
///
/// Guarded like [`set_state_class`]: the `ClassList` is only dereferenced
/// mutably when something actually moves, so a settled cell does not wake the
/// style engine every frame.
pub fn set_role_class(list: &mut Mut<'_, ClassList>, color: Color) {
    let wanted = role_class(color);
    for class in [TEXT_CLASS, TITLE_CLASS, HEADING_CLASS, DISABLED_TEXT_CLASS] {
        set_state_class(list, class, wanted == Some(class));
    }
}

/// Add or remove a state class, touching the [`ClassList`] only when the state
/// actually changed.
///
/// The guard is load-bearing, not tidiness. `Mut<ClassList>` marks the
/// component changed on **any** mutable deref, and the style engine re-resolves
/// what it is told has changed — so an unguarded `add` every frame would cost
/// more than the per-frame colour write this whole change removes, and would
/// hide the win behind a wash. Reading through the immutable `Deref` first
/// keeps the change tick clean on the frames nothing moved.
pub fn set_state_class(list: &mut Mut<'_, ClassList>, class: &'static str, wanted: bool) {
    if list.contains(class) == wanted {
        return;
    }
    if wanted {
        list.add(class);
    } else {
        list.remove(class);
    }
}

/// [`set_state_class`] for a node the caller knows only by [`Entity`] — the
/// shape a widget needs for its *label*, which is a separate node from the
/// surface carrying the state.
///
/// A node with no [`ClassList`] is skipped rather than given one: a widget that
/// wants to be styled says so when it spawns, and silently growing a list here
/// would make a missing class at the spawn site look like it worked.
pub fn set_state_class_on<F: bevy::ecs::query::QueryFilter>(
    classes: &mut Query<'_, '_, &mut ClassList, F>,
    node: Entity,
    class: &'static str,
    wanted: bool,
) {
    if let Ok(mut list) = classes.get_mut(node) {
        set_state_class(&mut list, class, wanted);
    }
}

/// What [`set_action_button_enabled`] reads: the filter saying whether a button
/// already carries [`InteractionDisabled`](bevy::ui::InteractionDisabled), so
/// the marker is toggled alone rather than re-marked changed every frame (which
/// would give the translation sweep and the layout gate work sixty times a
/// second over a window where nothing moved).
pub type DisabledButtons<'w, 's> = Query<'w, 's, (), With<bevy::ui::InteractionDisabled>>;

/// Mark one action button enabled or disabled.
///
/// Bevy's `InteractionDisabled` is **advisory**: it stops a window's own press
/// observer (each one filters on it) and paints nothing. The greying is the
/// skin's, through `.sk-button:disabled` and `.sk-button:disabled .sk-text`, so
/// this sets the marker and stops.
///
/// Shared because a disabled action button is not one window's idea: the
/// environment windows, the Friends pane and the Groups pane all grey the same
/// kind of button on the same kind of predicate, and a copy per window is a
/// place for the marker half to drift out of step with the press refusal.
pub fn set_action_button_enabled(
    commands: &mut Commands<'_, '_>,
    disabled: &DisabledButtons<'_, '_>,
    entity: Entity,
    enabled: bool,
) {
    if disabled.contains(entity) == enabled {
        if enabled {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        } else {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
    }
}

/// What [`set_button_on`] reads: whether a button already carries
/// `bevy_ui::Checked`, for the reason [`DisabledButtons`] exists — a marker
/// re-inserted every frame is a change every frame.
pub type CheckedButtons<'w, 's> = Query<'w, 's, (), With<bevy::ui::Checked>>;

/// Turn one **toggle** button on or off: a bottom-toolbar button whose floater
/// is open, the day-cycle track being edited, the Photo Tools time preset in
/// force.
///
/// A toggle that is on is `:checked` — `bevy_ui::Checked`, the engine state a
/// tab, a tick box and a menu tick already use, which `bevy_flair` syncs to the
/// pseudo-class — not a class of our own. It used to be `.sk-active`, a name
/// that says "held down" to anyone who knows CSS
/// (`viewer-skin-active-class-means-selected-not-pressed`), and the marker is
/// what an accessibility tree reads as a pressed toggle as well.
pub fn set_button_on(
    commands: &mut Commands<'_, '_>,
    checked: &CheckedButtons<'_, '_>,
    entity: Entity,
    on: bool,
) {
    if checked.contains(entity) != on {
        if on {
            commands.entity(entity).insert(bevy::ui::Checked);
        } else {
            commands.entity(entity).remove::<bevy::ui::Checked>();
        }
    }
}

/// The CSS class on a vertical scrollbar's frame — the column holding its
/// arrows and groove, `--scrollbar-thickness` wide. Every scrollbar in the
/// viewer is the one [`crate::scrollbar`] widget, so one rule reaches all of
/// them.
pub const SCROLLBAR_VERTICAL_CLASS: &str = "sk-scrollbar-vertical";

/// The CSS class on a horizontal scrollbar's frame — the row holding its
/// arrows and groove, `--scrollbar-thickness` tall.
pub const SCROLLBAR_HORIZONTAL_CLASS: &str = "sk-scrollbar-horizontal";

/// The CSS class on the square where a vertical and a horizontal scrollbar
/// meet (`--scrollbar-track`, `--scrollbar-thickness` square).
pub const SCROLLBAR_CORNER_CLASS: &str = "sk-scrollbar-corner";

/// The CSS class on each of a horizontal tab strip's four overflow buttons
/// (`--tab-scroll-bg`). Named here because its `:hover` rule puts it in
/// `HOVER_CLASSES`.
pub const TAB_SCROLL_BUTTON_CLASS: &str = "sk-tab-scroll-button";

/// The CSS class on a scrollbar's groove, the part the thumb runs in
/// (`--scrollbar-track`).
pub const SCROLLBAR_TRACK_CLASS: &str = "sk-scrollbar-track";

/// The CSS class on a scrollbar's thumb (`--scrollbar-thumb`, and
/// `--scrollbar-thumb-hover` under the pointer).
pub const SCROLLBAR_THUMB_CLASS: &str = "sk-scrollbar-thumb";

/// The CSS class on both of a scrollbar's arrow ends
/// (`--scrollbar-arrow-bg`). Its `display` is the `--scrollbar-arrows` token,
/// which is how a skin turns the ends on.
pub const SCROLLBAR_ARROW_CLASS: &str = "sk-scrollbar-arrow";

/// The CSS class on the arrow that steps toward the start, beside
/// [`SCROLLBAR_ARROW_CLASS`] — what the glyph rule selects the `▲` by.
pub const SCROLLBAR_ARROW_UP_CLASS: &str = "sk-scrollbar-arrow-up";

/// The CSS class on the arrow that steps toward the end — the `▼`.
pub const SCROLLBAR_ARROW_DOWN_CLASS: &str = "sk-scrollbar-arrow-down";

/// The CSS class on a horizontal bar's arrow that steps left — the `◀`. A
/// horizontal bar is physical, so this never mirrors.
pub const SCROLLBAR_ARROW_LEFT_CLASS: &str = "sk-scrollbar-arrow-left";

/// The CSS class on a horizontal bar's arrow that steps right — the `▶`.
pub const SCROLLBAR_ARROW_RIGHT_CLASS: &str = "sk-scrollbar-arrow-right";

/// The CSS class on an arrow's glyph host, whose `::before` `content` is the
/// arrow a skin draws (`--scrollbar-arrow`).
pub const SCROLLBAR_ARROW_GLYPH_CLASS: &str = "sk-scrollbar-arrow-glyph";

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

/// The CSS class on a checkbox — the row carrying `Checkbox`, and so the
/// `:checked` / `:disabled` its box and tick rules reach down from.
///
/// The checkbox's three classes live here with the rest of the state
/// vocabulary rather than in `sl-viewer-ui-widgets`'s `ui_checkbox`, so a rule
/// in `common.css` and the class that selects it are named in one crate.
pub const CHECKBOX_CLASS: &str = "sk-checkbox";

/// The CSS class on a checkbox's box: the frame and fill a tick sits in.
pub const CHECKBOX_BOX_CLASS: &str = "sk-checkbox-box";

/// The CSS class on a checkbox's tick. Its glyph is the skin's `content` on a
/// `::before`, so the widget's tick node carries `PseudoElementsSupport`.
pub const CHECKBOX_TICK_CLASS: &str = "sk-checkbox-tick";

/// The skin-driven text-caret and selection colours of one editable text field
/// (R28), written by the `caret-color` / `selection-color` /
/// `unfocused-selection-color` CSS properties (the `.sk-text-field` rule in
/// `common.css`, whose values are the `--caret` / `--selection` /
/// `--selection-unfocused` role tokens) and folded into the field's
/// [`bevy::text::TextCursorStyle`] by
/// `ui_text_input::drive_caret_blink`.
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
pub struct SkinTextCaret {
    /// The caret (text cursor) colour.
    pub caret: Color,
    /// The background colour of selected text while the field is focused.
    pub selection: Color,
    /// The background colour of selected text while the field is unfocused.
    pub selection_unfocused: Color,
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
///
/// Public for the same reason
/// [`register_palette_properties`](crate::skin_palette::register_palette_properties)
/// is: the shipped-skin tests drive the real stylesheets through a bare
/// `FlairPlugin` app rather than through [`ViewerSkinPlugin`], which wants the
/// whole UI scaffold under it, and an unregistered property does not fail — the
/// rule simply parses to nothing, and the assertion would read the fallback.
pub fn register_caret_properties(app: &mut App) {
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

/// The skin-driven transcript **band** colours of the Conversations floater —
/// the reference viewer's three-way message-class distinction in IM / group
/// windows: recalled local transcript lines, server-fetched session history,
/// and live messages. Written by the `chat-recall-color` /
/// `chat-server-history-color` / `chat-live-color` CSS properties (the
/// `.sk-conversations` rule in `common.css`, whose values are the
/// `--chat-recall` / `--chat-server-history` / `--chat-live` role tokens) and
/// read by `conversations::refresh_conversations` when it rebuilds a
/// transcript pane.
///
/// The same shim pattern as [`SkinTextCaret`]: transcript lines are spawned
/// dynamically with per-line `linkified_text::LinkTextStyle` colours,
/// which `bevy_flair` cannot drive directly, so the skin lands the resolved
/// colours here (on the floater root) and the rebuild reads them off.
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub struct SkinChatBands {
    /// Local transcript recall (the reference `ChatHistoryMessageFromLog`).
    pub recall: Color,
    /// Server-fetched session history (the reference — Firestorm-only —
    /// `ChatHistoryMessageFromServerLog`).
    pub server_history: Color,
    /// Live messages (the reference `AgentChatColor`).
    pub live: Color,
}

impl Default for SkinChatBands {
    /// The reference colour values, used unchanged by the stock skins and as
    /// the fallback for an unskinned pane: mid grey `0.5 0.5 0.5` for local
    /// recall, muted green `0.37 0.51 0.38` for server history
    /// (`fschathistory.cpp:1566-1582`), white for live lines.
    fn default() -> Self {
        Self {
            recall: Color::srgb(0.5, 0.5, 0.5),
            server_history: Color::srgb(0.37, 0.51, 0.38),
            live: Color::WHITE,
        }
    }
}

/// Register the Conversations transcript-band CSS properties on the
/// `bevy_flair` registry, mapping the three `chat-*-color` properties onto
/// [`SkinChatBands`]'s fields. Runs in `build`, before the CSS asset loader
/// snapshots the registry at plugin `finish`.
fn register_chat_band_properties(app: &mut App) {
    app.register_component_properties::<SkinChatBands>();
    let css = app.world().resource::<CssPropertyRegistry>();
    css.register_property(
        "chat-recall-color",
        SkinChatBands::property_field_ref("recall"),
    );
    css.register_property(
        "chat-server-history-color",
        SkinChatBands::property_field_ref("server_history"),
    );
    css.register_property("chat-live-color", SkinChatBands::property_field_ref("live"));
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
pub struct SkinMargin {
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
pub struct SkinPadding {
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
/// `bevy_flair` natively. A one-colour frame has no handedness; a bevel's
/// per-side colours do, and are deliberately *not* logical — see
/// [`BANNED_PHYSICAL_PROPERTIES`].)
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub struct SkinBorder {
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
pub struct SkinInset {
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
pub struct SkinRadius {
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

/// The nodes `resolve_skin_boxes` has work for: any carrying a skin box that
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

/// Mark every skin box dirty when [`UiDirection`] flips, so `resolve_skin_boxes`
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
/// Enforced at build time by the shipped-skin check (a skin that uses one fails
/// the build). Public because that check lives in the crate that owns
/// `assets/skins/` — the binary — and because the
/// `viewer-ui-skin-l10n-functions` / user-skin follow-up wants the same scan at
/// run time for user-authored skins.
///
/// # The bevel exception (`viewer-skin-bevel-border-policy`)
///
/// `border-left-color` / `border-right-color` are banned for a different reason
/// from the rest, and have no logical spelling to offer instead. Their only use
/// is a **bevel** — a lit edge and a shaded one — and a bevel's light source is
/// a property of the rendering, not of the writing direction: mirrored under an
/// RTL locale it would look lit from the wrong corner, which is why no desktop
/// toolkit mirrors one. So the per-side colours are **physical on purpose**,
/// and the structural sheet (`skins/common.css`) is the one place that writes
/// them: `.sk-button` and the two field wells read `--button-bevel-top-left` /
/// `--button-bevel-bottom-right` / `--field-bevel-top-left` /
/// `--field-bevel-bottom-right`, and a skin draws a bevel by setting those
/// tokens. Keeping the side properties banned *in a skin* means one rule owns
/// the handedness, rather than every skin re-deciding it; a test in this module
/// holds `common.css` to writing them from bevel tokens only.
///
/// The tokens name the physical edges rather than "light" and "shadow" because
/// which corner is lit is itself the skin's choice, and differs per widget:
/// the Windows convention raises a button and sinks a field (opposite
/// corners), while Vintage draws both dark at the top-left.
pub const BANNED_PHYSICAL_PROPERTIES: &[&str] = &[
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
#[must_use]
pub fn logical_replacement(physical: &str) -> Option<&'static str> {
    let replacement = match physical {
        "margin-left" | "margin-right" => "margin-inline-start / margin-inline-end",
        "padding-left" | "padding-right" => "padding-inline-start / padding-inline-end",
        "border-left-width" | "border-right-width" => {
            "border-inline-start-width / border-inline-end-width"
        }
        "border-left-color" | "border-right-color" => {
            "border-color for a one-colour frame; for a bevel, the \
             --button-bevel-* / --field-bevel-* tokens (a light source is \
             physical and must not mirror, so common.css owns the sides)"
        }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannedProperty {
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
#[must_use]
pub fn scan_banned_properties(css: &str) -> Vec<BannedProperty> {
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
        BANNED_PHYSICAL_PROPERTIES, DEFAULT_SKIN, FOCUSABLE_CLASS, HOVER_CLASSES, LIST_ROW_CLASS,
        SkinMargin, SkinRadius, SkinSelection, TABLE_ROW_CLASS, UiDirection, invalidate_skin_boxes,
        logical_replacement, resolve_skin_boxes, scan_banned_properties, stamp_focus_ring_class,
        stamp_hover_state,
    };
    use crate::skin_palette::SkinPalette;
    use bevy::input_focus::tab_navigation::TabIndex;
    use bevy::prelude::*;
    use bevy_flair::style::components::ClassList;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;
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

    /// **The structural sheet writes a side colour only to draw a bevel.**
    ///
    /// `common.css` is not a skin and is not scanned by
    /// [`scan_banned_properties`]: it is the one sheet allowed the physical
    /// side colours, because a bevel's light source must not mirror (see
    /// [`BANNED_PHYSICAL_PROPERTIES`]). That licence is narrow, and this holds
    /// it there — every per-side colour it declares reads a `--*-bevel-*`
    /// token, and it declares no *other* banned property at all, so a physical
    /// margin or radius cannot slip in under the bevel's cover.
    #[test]
    fn common_css_writes_side_colours_only_from_bevel_tokens() {
        const COMMON_CSS: &str = include_str!("skins/common.css");
        const SIDE_COLOURS: [&str; 4] = [
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ];
        let mut bevel_sides = 0_usize;
        for (index, line) in COMMON_CSS.lines().enumerate() {
            let Some((head, value)) = line.split_once(':') else {
                continue;
            };
            let name = head.trim();
            if SIDE_COLOURS.contains(&name) {
                let value = value.trim().trim_end_matches(';');
                assert!(
                    value.starts_with("var(--") && value.contains("-bevel-"),
                    "common.css:{}: `{name}: {value}` — a side colour that is not a \
                     bevel token has handedness no skin chose",
                    index.saturating_add(1)
                );
                bevel_sides = bevel_sides.saturating_add(1);
            } else {
                assert!(
                    !BANNED_PHYSICAL_PROPERTIES.contains(&name),
                    "common.css:{}: `{name}` is banned in a skin and has no bevel \
                     excuse here either",
                    index.saturating_add(1)
                );
            }
        }
        // The button at rest, held down (the bevel turned inside out) and
        // refused (the resting bevel restated over a press), and the two field
        // wells: four sides each.
        assert_eq!(bevel_sides, 20, "the bevel rules moved or lost a side");
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

    /// **Everything a `:hover` rule selects is given the component that
    /// pseudo-class is read from.**
    ///
    /// `bevy_picking`'s `Hovered` is opt-in and nothing adds it for you, so
    /// `common.css`'s `.sk-list-row:hover` matched nothing at all until the
    /// scaffold stamped it — a rule that parsed, resolved and painted never.
    /// The cascade test for that rule cannot catch this: it inserts `Hovered`
    /// itself, which is precisely the state nothing was supplying.
    ///
    /// Anything that already carries `Interaction` is left alone: `bevy_ui`'s
    /// `Button` requires it, `bevy_flair` reads it for the same pseudo-state,
    /// and two systems writing one state is a race to nowhere.
    #[test]
    fn a_hoverable_node_is_given_the_hover_component() {
        use bevy::picking::hover::Hovered;

        let mut app = App::new();
        app.add_systems(Update, stamp_hover_state);

        let list_row = app
            .world_mut()
            .spawn(ClassList::new_with_classes([LIST_ROW_CLASS]))
            .id();
        let table_row = app
            .world_mut()
            .spawn(ClassList::new_with_classes([TABLE_ROW_CLASS]))
            .id();
        let button_row = app
            .world_mut()
            .spawn((
                ClassList::new_with_classes([LIST_ROW_CLASS]),
                Interaction::default(),
            ))
            .id();
        let nothing_hoverable = app
            .world_mut()
            .spawn(ClassList::new_with_classes(["sk-panel"]))
            .id();

        app.update();

        assert!(
            app.world().get::<Hovered>(list_row).is_some(),
            "a hand-rolled list's row never hovers without this"
        );
        assert!(
            app.world().get::<Hovered>(table_row).is_some(),
            "a table row never hovers without this"
        );
        assert!(
            app.world().get::<Hovered>(button_row).is_none(),
            "a node that already carries `Interaction` has the pseudo-state \
             driven for it"
        );
        assert!(
            app.world().get::<Hovered>(nothing_hoverable).is_none(),
            "a class with no `:hover` rule gains nothing — the component is \
             only free while the set stays the set the stylesheet names"
        );
    }

    /// Trigger a pointer event on an entity by hand and settle a frame — the
    /// observer call a real press makes, without a picking backend to aim one.
    /// A macro because `World::trigger` wants the concrete `Pointer<E>`.
    macro_rules! trigger_pointer {
        ($app:expr, $entity:expr, $event:expr $(,)?) => {{
            let location = bevy::picking::pointer::Location {
                target: bevy::camera::NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            };
            $app.world_mut().trigger(Pointer::new(
                bevy::picking::pointer::PointerId::Mouse,
                location,
                $event,
                $entity,
            ));
            $app.update();
        }};
    }

    /// **A plain button box is held down by a press and let up by the release,**
    /// which is the whole of what `:active` reads.
    ///
    /// Toggled on one entity rather than spawned once per state, because the
    /// failure this guards is a state that arrives and never leaves (or never
    /// arrives), and only a toggle sees both halves. A refused box never goes
    /// down, a secondary press does not hold it, and a node that brings a
    /// button component of its own is left to it.
    #[test]
    fn a_plain_button_box_is_held_down_while_pressed() {
        use super::{
            ACTION_BUTTON_CLASS, PressTracked, TOOLBAR_BUTTON_CLASS, press_tracked_on_cancel,
            press_tracked_on_drag_end, press_tracked_on_press, press_tracked_on_release,
            stamp_press_state,
        };
        use bevy::picking::backend::HitData;
        use bevy::picking::events::{DragEnd, Press, Release};
        use bevy::picking::pointer::PointerButton;
        use bevy::ui::{InteractionDisabled, Pressed};

        let mut app = App::new();
        app.add_systems(Update, stamp_press_state)
            .add_observer(press_tracked_on_press)
            .add_observer(press_tracked_on_release)
            .add_observer(press_tracked_on_drag_end)
            .add_observer(press_tracked_on_cancel);

        let toolbar = app
            .world_mut()
            .spawn(ClassList::new_with_classes([TOOLBAR_BUTTON_CLASS]))
            .id();
        let refused = app
            .world_mut()
            .spawn((
                ClassList::new_with_classes([ACTION_BUTTON_CLASS]),
                InteractionDisabled,
            ))
            .id();
        let widget = app
            .world_mut()
            .spawn((
                ClassList::new_with_classes([ACTION_BUTTON_CLASS]),
                bevy::ui_widgets::Button,
            ))
            .id();
        let legacy = app
            .world_mut()
            .spawn((
                ClassList::new_with_classes([ACTION_BUTTON_CLASS]),
                Interaction::default(),
            ))
            .id();
        let panel = app
            .world_mut()
            .spawn(ClassList::new_with_classes(["sk-panel"]))
            .id();
        app.update();

        assert!(app.world().get::<PressTracked>(toolbar).is_some());
        assert!(app.world().get::<PressTracked>(refused).is_some());
        assert!(
            app.world().get::<PressTracked>(widget).is_none(),
            "a `bevy_ui_widgets` button keeps its own `Pressed`"
        );
        assert!(
            app.world().get::<PressTracked>(legacy).is_none(),
            "an `Interaction` button's press is synced from that"
        );
        assert!(app.world().get::<PressTracked>(panel).is_none());

        let press = |button| Press {
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        };
        let release = || Release {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        };
        let held = |app: &App, entity| app.world().get::<Pressed>(entity).is_some();

        trigger_pointer!(app, toolbar, press(PointerButton::Secondary));
        assert!(!held(&app, toolbar), "a secondary press does not hold it");

        trigger_pointer!(app, toolbar, press(PointerButton::Primary));
        assert!(held(&app, toolbar), "the press did not hold the box down");
        trigger_pointer!(app, toolbar, release());
        assert!(!held(&app, toolbar), "the release did not let it up");

        trigger_pointer!(app, toolbar, press(PointerButton::Primary));
        assert!(held(&app, toolbar));
        trigger_pointer!(
            app,
            toolbar,
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::new(40.0, 0.0),
            },
        );
        assert!(
            !held(&app, toolbar),
            "a drag off the box and a release elsewhere left it down"
        );

        trigger_pointer!(app, refused, press(PointerButton::Primary));
        assert!(!held(&app, refused), "a refused button must not go down");
    }

    /// The class the pseudo-class at `at` in `css` hangs off, walking back over
    /// any pseudo-classes compounded before it — `.sk-button:checked:active`
    /// names `sk-button`.
    fn class_before(css: &str, at: usize) -> Option<&str> {
        let mut before = css.get(..at)?;
        loop {
            let head =
                before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '-' || c == '_');
            if let Some(pseudo) = head.strip_suffix(':') {
                before = pseudo;
                continue;
            }
            let class = before.get(head.len()..)?;
            return (head.ends_with('.') && !class.is_empty()).then_some(class);
        }
    }

    /// **Every `:active` rule in `common.css` selects something that can
    /// actually be pressed,** and every class stamped for a press has a rule —
    /// the press twin of `every_hover_rule_has_something_to_hover`, for the same
    /// reason: a cascade test supplies `Pressed` itself and cannot see that
    /// nothing else does.
    #[test]
    fn every_press_rule_has_something_to_press() {
        use super::PRESS_CLASSES;

        let css = strip_css_comments(include_str!("skins/common.css"));
        let mut named: Vec<&str> = css
            .match_indices(":active")
            .filter_map(|(at, _)| class_before(&css, at))
            .collect();
        named.sort_unstable();
        named.dedup();
        let mut declared: Vec<&str> = PRESS_CLASSES.to_vec();
        declared.sort_unstable();
        assert_eq!(
            named, declared,
            "`common.css` writes an `:active` rule for a class `stamp_press_state` \
             does not stamp, or stamps one no rule presses any more"
        );
        let missing: Vec<&str> = PRESS_CLASSES
            .iter()
            .copied()
            .filter(|class| {
                !css.contains(&format!(".{class} {{")) && !css.contains(&format!(".{class},"))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "these classes have an `:active` rule and no resting rule, so the \
             first press would stick: {missing:?}"
        );
    }

    /// **No class in the skin vocabulary shares its name with a CSS
    /// pseudo-class that means something else.** `.sk-active` meant *selected*
    /// and was read as *pressed* by anyone who knows CSS; a class that is a
    /// pseudo-class's name is the same trap, whatever it is meant to mean.
    #[test]
    fn no_class_is_named_after_a_pseudo_class() {
        // The pseudo-classes whose *meaning* a class could be mistaken for.
        // A longer name that merely starts with one (`.sk-disabled-surface`,
        // `.sk-focus-within`) says what it is and is not a clash.
        const PSEUDO_CLASSES: &[&str] = &[
            "active", "hover", "focus", "checked", "enabled", "visited", "target",
        ];
        let css = strip_css_comments(include_str!("skins/common.css"));
        let clashing: Vec<&str> = PSEUDO_CLASSES
            .iter()
            .copied()
            .filter(|name| {
                let class = format!(".sk-{name}");
                css.match_indices(&class).any(|(at, _)| {
                    css.get(at.saturating_add(class.len())..)
                        .and_then(|rest| rest.chars().next())
                        .is_none_or(|next| !(next.is_alphanumeric() || next == '-' || next == '_'))
                })
            })
            .collect();
        assert!(
            clashing.is_empty(),
            "`common.css` has a class spelled like a pseudo-class: {clashing:?}"
        );
    }

    /// `css` with every `/* … */` comment removed, so a scan over selectors
    /// does not read the prose around them.
    fn strip_css_comments(css: &str) -> String {
        let mut out = String::new();
        let mut rest = css;
        while let Some((before, after)) = rest.split_once("/*") {
            out.push_str(before);
            let Some((_comment, tail)) = after.split_once("*/") else {
                return out;
            };
            rest = tail;
        }
        out.push_str(rest);
        out
    }

    /// **Every `:hover` rule in `common.css` selects something that can
    /// actually be hovered.**
    ///
    /// This is the check that would have caught three dead rules, and the one
    /// the cascade tests structurally cannot be: they supply `Hovered`
    /// themselves. It reads the stylesheet, collects the class of every
    /// `:hover` selector in it, and asserts that set is **exactly**
    /// [`HOVER_CLASSES`] — the list `stamp_hover_state` walks.
    ///
    /// Both directions matter. A rule whose class is missing from the list
    /// paints nothing, silently and only in the live viewer. A class in the
    /// list with no rule left means the stamp is handing out a component for a
    /// hover nobody draws, which is how the list would rot into a
    /// catch-everything.
    #[test]
    fn every_hover_rule_has_something_to_hover() {
        // Comments first, or the prose explaining a rule counts as one — and
        // this file explains every rule it has, including two it no longer
        // carries.
        let css = strip_css_comments(include_str!("skins/common.css"));
        let mut named: Vec<&str> = Vec::new();
        for (at, _) in css.match_indices(":hover") {
            let Some(before) = css.get(..at) else {
                continue;
            };
            // The class is the identifier the pseudo-class hangs off: walk back
            // over the name to the `.` that starts it.
            let head =
                before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '-' || c == '_');
            let Some(class) = before.get(head.len()..) else {
                continue;
            };
            if head.ends_with('.') && !class.is_empty() {
                named.push(class);
            }
        }
        named.sort_unstable();
        named.dedup();
        let mut declared: Vec<&str> = HOVER_CLASSES.to_vec();
        declared.sort_unstable();
        assert_eq!(
            named, declared,
            "`common.css` writes a `:hover` rule for a class `stamp_hover_state` \
             does not stamp (so the rule paints nothing at all, in the live \
             viewer only), or stamps one no rule uses any more"
        );
    }

    /// **Every `:hover` rule has a resting rule to fall back to.**
    ///
    /// `bevy_flair` does not *revert* a property when a rule stops matching — it
    /// applies the winning rule's value and nothing more. A class whose only
    /// rule is a `:hover` therefore paints on the way in and has nothing to
    /// paint on the way out, so the state sticks for the life of the node. That
    /// is what `.sk-tile` did: the wash stayed on every emoji cell the pointer
    /// had ever crossed, which a dense grid makes obvious within seconds and no
    /// test here could see.
    ///
    /// The check is deliberately coarse — a bare `.<class>` rule exists — rather
    /// than per-property. It catches the shape of the mistake (a state with no
    /// resting counterpart at all), which is the one that has actually been
    /// made, twice.
    #[test]
    fn every_hover_rule_has_a_resting_rule() {
        let css = strip_css_comments(include_str!("skins/common.css"));
        let missing: Vec<&str> = HOVER_CLASSES
            .iter()
            .copied()
            .filter(|class| {
                // A selector list writes `.a,\n.b {`, so either spelling counts.
                !css.contains(&format!(".{class} {{")) && !css.contains(&format!(".{class},"))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "these classes have a `:hover` rule and no resting rule, so once \
             the pointer has touched one the state never comes off it: \
             {missing:?}"
        );
    }

    /// `resolve` prefers the CLI pair atomically, falls back to a validated
    /// stored pair, and defaults last. (Runs without the `SL_VIEWER_SKIN` /
    /// `SL_VIEWER_THEME` env overrides set, as the test environment does not
    /// set them.)
    #[test]
    fn resolve_prefers_cli_then_validated_stored_pair() {
        // A stored pair passes through.
        let stored = SkinSelection::resolve(
            None,
            None,
            Some("graphite".to_owned()),
            Some("dark".to_owned()),
        );
        assert_eq!(stored.skin, "graphite");
        assert_eq!(stored.theme.as_deref(), Some("dark"));

        // A stored theme the stored skin does not ship is dropped, and an
        // unknown stored skin falls back to the default.
        let invalid_theme = SkinSelection::resolve(
            None,
            None,
            Some("azure".to_owned()),
            Some("dark".to_owned()),
        );
        assert_eq!(invalid_theme.skin, "azure");
        assert_eq!(invalid_theme.theme, None);
        let unknown_skin =
            SkinSelection::resolve(None, None, Some("no-such-skin".to_owned()), None);
        assert_eq!(unknown_skin.skin, DEFAULT_SKIN);

        // A CLI skin takes the whole pair: the stored theme must not ride
        // along onto a different skin.
        let cli = SkinSelection::resolve(
            Some("azure".to_owned()),
            None,
            Some("graphite".to_owned()),
            Some("dark".to_owned()),
        );
        assert_eq!(cli.skin, "azure");
        assert_eq!(cli.theme, None);
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

    /// **A role's value must name that role and no other.**
    ///
    /// [`role_class`](super::role_class) reads a role back out of a colour, which is sound only
    /// while each of the four values is unique across the whole palette — two
    /// roles sharing a value would make a label take the wrong class, silently
    /// and only under a skin that paints those two classes differently.
    ///
    /// Checked by reflection over every field rather than against a list, so a
    /// role added later is covered without anyone remembering to add it here.
    #[test]
    fn the_text_roles_are_the_only_roles_with_their_values() {
        use bevy::reflect::structs::Struct as _;

        let fallback = SkinPalette::FALLBACK;
        let keyed = [
            ("text_primary", fallback.text_primary),
            ("text_muted", fallback.text_muted),
            ("text_heading", fallback.text_heading),
            ("text_disabled", fallback.text_disabled),
        ];
        for (role, value) in keyed {
            let named: Vec<&str> = (0..fallback.field_len())
                .filter_map(|index| {
                    let name = fallback.name_at(index)?;
                    let color = fallback.field_at(index)?.try_downcast_ref::<Color>()?;
                    (*color == value).then_some(name)
                })
                .collect();
            // Without this the test passes vacuously if the walk downcasts
            // nothing — the shape of a check that checks nothing.
            assert!(
                named.contains(&role),
                "the reflection walk did not find `{role}` itself, so it is \
                 looking at no colours and would report no clash whatever the \
                 palette said"
            );
            let clashes: Vec<&str> = named.into_iter().filter(|name| *name != role).collect();
            assert!(
                clashes.is_empty(),
                "`{role}` shares its value with {clashes:?}, so a label coloured \
                 with it can no longer be mapped back to one role — give the \
                 roles distinct fallback values, or replace the derivation in \
                 `role_class` with an explicit role at the call sites"
            );
        }
    }
}
