//! The **linkified-text widget** (`viewer-url-linkification`, rendering half): the
//! reusable `bevy_ui` element that renders a run of text with its `http(s)` URLs,
//! SLURLs and `secondlife:///app/...` entity links turned into coloured,
//! hoverable, clickable spans. Consumers (nearby chat, IM, notifications,
//! profiles, object descriptions) drop this in place of a bare
//! [`Text`](bevy::prelude::Text).
//!
//! # How the text is laid out
//!
//! The source string is split into segments by [`crate::url_linkify::linkify`],
//! and each segment becomes its own child of a **wrapping row**
//! ([`FlexWrap::Wrap`]): plain runs are non-interactive `Text` nodes, links are
//! `Button` + `Text` nodes. A URL is a single node, so it never breaks
//! mid-link; a long plain run is width-bounded so it wraps as a block. This keeps
//! per-link picking, hover and click rock-solid (each link is a real node) while
//! parley shapes each run natively. (The reference `LLTextBase` hit-tests glyph
//! rects on one laid-out block; a per-node row is the `bevy_ui`-idiomatic
//! equivalent and is what the short chat / notice / profile contexts need.)
//!
//! # Three interactions
//!
//! - **Resolve** — an agent / group link starts as a "(loading…)" placeholder and
//!   is rewritten in place once the name cache answers, exactly like
//!   [`crate::ui_name_link`]; the name is requested once on spawn.
//! - **Hover** — resting on a link shows its **actual target URL** in a tooltip,
//!   so the user can vet where a `[shortened text]` or entity link really goes
//!   before clicking.
//! - **Click** — a `Web` link opens directly (the embedded browser for a trusted
//!   Second Life host, the system browser for an external one — the reference
//!   internal-vs-external split); every click also emits [`LinkActivated`] so the
//!   SLURL dispatcher ([[viewer-slurl-parse-dispatch]]) and any consumer can route
//!   the [`LinkTarget`].
//!
//! Reference (Firestorm, read-only): `llui/lltextbase` (segment rendering + hit
//! testing), `llui/llurlaction` (the click actions).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bevy::window::PrimaryWindow;

use sl_client_bevy::{AgentKey, Command, GroupKey, SlCommand};

use crate::avatars::{AvatarState, NameRecord};
use crate::groups::GroupsModel;
use crate::i18n::Translator;
use crate::parcel_names::ParcelNames;
use crate::ui::UiRoot;
use crate::ui_element::{ElementCx, TextMayClip};
use crate::ui_font::UiFont;
use crate::ui_name_link::{NAME_LINK_COLOR, NAME_PLAIN_COLOR};
use crate::url_linkify::{AgentNameStyle, LinkIcon, LinkLabel, LinkTarget, TextRun, linkify};
use crate::web_floater::{OpenWebBrowser, open_in_system_browser};

/// The leading-icon size, in logical pixels, relative to the label font size.
const ICON_SCALE: f32 = 1.0;

/// The gap between a link's icon and its label, in logical pixels.
const ICON_GAP: f32 = 3.0;

/// The bundled icon asset paths, tinted at runtime to the link colour (the same
/// white-mask convention as the parcel-flag icons).
const ICON_AGENT_PATH: &str = "icons/link/agent.png";
/// The group-link icon asset path.
const ICON_GROUP_PATH: &str = "icons/link/group.png";
/// The location / SLURL icon asset path.
const ICON_LOCATION_PATH: &str = "icons/link/location.png";

/// The Fluent key of the "(loading…)" placeholder a name-resolving link shows
/// until its name cache answers (the reference `AvatarNameWaiting`).
const LOADING_KEY: &str = "link-loading";

/// The default label font size, in logical pixels.
const DEFAULT_FONT_SIZE: f32 = 14.0;

/// The tooltip's background, used when no skin overrides it.
const TOOLTIP_BACKGROUND: Color = Color::srgba(0.06, 0.07, 0.10, 0.97);

/// The tooltip's border colour.
const TOOLTIP_BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// The tooltip's text colour.
const TOOLTIP_TEXT: Color = Color::srgb(0.90, 0.92, 0.96);

/// The tooltip's text size, in logical pixels.
const TOOLTIP_FONT_SIZE: f32 = 12.0;

/// The tooltip's offset from the cursor, in logical pixels.
const TOOLTIP_CURSOR_OFFSET: Vec2 = Vec2::new(14.0, 18.0);

// ---------------------------------------------------------------------------
// Public spawn API + style.
// ---------------------------------------------------------------------------

/// How a linkified-text block is styled: its font size and its two text colours.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkTextStyle {
    /// The text size, in logical pixels.
    pub(crate) font_size: f32,
    /// The colour of plain (non-link) text.
    pub(crate) plain_color: Color,
    /// The colour of a link.
    pub(crate) link_color: Color,
}

impl Default for LinkTextStyle {
    /// The resting style: the default size and the shared name-link colours.
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            plain_color: NAME_PLAIN_COLOR,
            link_color: NAME_LINK_COLOR,
        }
    }
}

impl LinkTextStyle {
    /// A style at `font_size` with the default colours.
    pub(crate) fn at(font_size: f32) -> Self {
        Self {
            font_size,
            ..Self::default()
        }
    }
}

/// Spawn a linkified-text block under `parent`: the source string is segmented,
/// each segment rendered as a child of a wrapping row, and the returned entity is
/// the row container (so a caller can width-bound or reparent it).
pub(crate) fn spawn_linkified_text(
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    style: LinkTextStyle,
) -> Entity {
    let container = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                ..default()
            },
            // The row itself never blocks picks; its plain children ignore picks
            // and its link children opt back in.
            Pickable::IGNORE,
            Name::new("linkified-text"),
            ChildOf(parent),
        ))
        .id();
    populate_linkified_text(commands, container, text, style);
    container
}

/// Segment `text` and spawn one child node per segment under `container`.
/// Public within the crate so a rich reader (the notecard body) can interleave
/// linkified prose runs with its own inline nodes in a shared wrapping row.
pub(crate) fn populate_linkified_text(
    commands: &mut Commands,
    container: Entity,
    text: &str,
    style: LinkTextStyle,
) {
    for run in linkify(text) {
        match run {
            TextRun::Plain(plain) => spawn_plain_run(commands, container, &plain, style),
            TextRun::Link(link) => spawn_link_run(commands, container, link, style),
        }
    }
}

/// Spawn a non-interactive plain-text run. Width-bounded so a long run wraps as a
/// block instead of overflowing the row.
fn spawn_plain_run(commands: &mut Commands, container: Entity, text: &str, style: LinkTextStyle) {
    commands.spawn((
        Text::new(text.to_owned()),
        UiFont::Sans.at(style.font_size),
        TextColor(style.plain_color),
        Node {
            max_width: Val::Percent(100.0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(container),
    ));
}

/// Spawn one clickable link run: a small horizontal box carrying the [`LinkNode`]
/// (and the hover / click observers), with an optional leading icon and the label
/// text as its children. Keeping the icon and label in one non-wrapping box means
/// they never split across a line break and a click anywhere on the box (icon or
/// text) activates the link.
fn spawn_link_run(
    commands: &mut Commands,
    container: Entity,
    link: crate::url_linkify::LinkMatch,
    style: LinkTextStyle,
) {
    let link_box = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(ICON_GAP),
                ..default()
            },
            Button,
            TabIndex(0),
            Pickable::default(),
            ChildOf(container),
        ))
        .id();
    // The optional leading icon: a fixed-size white-mask image, tinted to the link
    // colour, whose handle is filled in by `apply_link_icons` (the spawn site has
    // no `AssetServer`).
    if link.icon != LinkIcon::None {
        commands.spawn((
            ImageNode {
                color: style.link_color,
                ..default()
            },
            Node {
                width: Val::Px(style.font_size * ICON_SCALE),
                height: Val::Px(style.font_size * ICON_SCALE),
                ..default()
            },
            LinkIconMarker(link.icon),
            Pickable::IGNORE,
            ChildOf(link_box),
        ));
    }
    let label_entity = commands
        .spawn((
            Text::new(link.label.fallback()),
            UiFont::Sans.at(style.font_size),
            TextColor(style.link_color),
            Pickable::IGNORE,
            ChildOf(link_box),
        ))
        .id();
    commands
        .entity(link_box)
        .insert(LinkNode {
            url: link.url,
            target: link.target,
            label: link.label,
            label_entity,
            tooltip_key: link.tooltip_key,
        })
        .observe(on_link_over)
        .observe(on_link_out)
        .observe(on_link_press);
}

// ---------------------------------------------------------------------------
// The per-link component + the click event.
// ---------------------------------------------------------------------------

/// A rendered link box: its canonical URL, its dispatch target, how its label
/// resolves, and the child text entity the resolved label is written into.
/// [`refresh_link_labels`] keeps that text in step with the name caches; the
/// observers read the URL / target for hover and click.
#[derive(Component, Debug, Clone)]
pub(crate) struct LinkNode {
    /// The canonical target URL — shown on hover, opened / dispatched on click.
    url: String,
    /// What the link points at.
    target: LinkTarget,
    /// How the visible label is produced.
    label: LinkLabel,
    /// The child [`Text`] entity the resolved label is written into.
    label_entity: Entity,
    /// The Fluent key of the tooltip category line (shown above the URL on hover).
    tooltip_key: &'static str,
}

/// Marks a link's leading-icon image so [`apply_link_icons`] can fill its handle
/// in once (the spawn site has no `AssetServer`). Carries which icon it wants.
#[derive(Component, Debug, Clone, Copy)]
struct LinkIconMarker(LinkIcon);

/// Emitted when a link is clicked, for the SLURL dispatcher and any consumer to
/// route. A `Web` target is additionally opened directly by
/// [`dispatch_web_links`], so a consumer that only cares about SLURLs can ignore
/// those.
#[derive(Message, Debug, Clone)]
pub(crate) struct LinkActivated {
    /// What the clicked link points at.
    pub(crate) target: LinkTarget,
    /// The clicked link's canonical URL.
    pub(crate) url: String,
}

// ---------------------------------------------------------------------------
// Observers: hover shows the URL, click activates.
// ---------------------------------------------------------------------------

/// On pointer-over, publish the hovered link's URL + tooltip category so
/// [`position_link_tooltip`] shows them.
fn on_link_over(
    over: On<Pointer<Over>>,
    links: Query<&LinkNode>,
    mut hovered: ResMut<HoveredLink>,
) {
    if let Ok(link) = links.get(over.entity) {
        hovered.link = Some(HoveredLinkInfo {
            url: link.url.clone(),
            tooltip_key: link.tooltip_key,
        });
    }
}

/// On pointer-out, clear the hovered link so the tooltip hides.
fn on_link_out(_out: On<Pointer<Out>>, mut hovered: ResMut<HoveredLink>) {
    hovered.link = None;
}

/// On a primary press, emit [`LinkActivated`] for the target — the SLURL
/// dispatcher and consumers route it, and [`dispatch_web_links`] opens `Web`
/// targets directly.
fn on_link_press(
    press: On<Pointer<Press>>,
    links: Query<&LinkNode>,
    mut activated: MessageWriter<LinkActivated>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(link) = links.get(press.entity) {
        activated.write(LinkActivated {
            target: link.target.clone(),
            url: link.url.clone(),
        });
    }
}

/// Open every activated `Web` link: a trusted Second Life host in the embedded
/// browser ([`OpenWebBrowser`]), an external host in the system browser — the
/// reference internal-vs-external distinction. Non-`Web` targets are left for the
/// SLURL dispatcher.
fn dispatch_web_links(
    mut activated: MessageReader<LinkActivated>,
    mut browsers: MessageWriter<OpenWebBrowser>,
) {
    for event in activated.read() {
        let LinkTarget::Web { trusted } = &event.target else {
            continue;
        };
        if *trusted {
            browsers.write(OpenWebBrowser {
                url: Some(event.url.clone()),
            });
        } else {
            open_in_system_browser(&event.url);
        }
    }
}

// ---------------------------------------------------------------------------
// Name resolution: keep agent / group labels in step with the caches.
// ---------------------------------------------------------------------------

/// The visible label for a link, given the live name caches (each optional, so
/// the widget also works before the session models exist — e.g. in the gallery).
/// A `Fixed` label always resolves; a name-resolving label falls back to the
/// loading placeholder until its cache holds the name.
fn label_text(
    label: &LinkLabel,
    avatars: Option<&AvatarState>,
    groups: Option<&GroupsModel>,
    parcels: Option<&ParcelNames>,
    translator: &Translator,
) -> String {
    let loading = || translator.get(LOADING_KEY);
    match label {
        LinkLabel::Fixed(text) => text.clone(),
        LinkLabel::Agent(agent, style) => avatars
            .and_then(|avatars| avatars.name_record(*agent))
            .and_then(|record| agent_label(record, *style))
            .unwrap_or_else(loading),
        LinkLabel::Group(group) => groups
            .and_then(|groups| groups.group_name(*group))
            .map_or_else(loading, str::to_owned),
        LinkLabel::Parcel(parcel) => parcels
            .and_then(|parcels| parcels.name_of(*parcel))
            .map_or_else(loading, str::to_owned),
    }
}

/// An agent's label in the requested style, from its name record — the reference
/// complete / display / username forms. `None` until a record with the needed
/// field arrives.
fn agent_label(record: &NameRecord, style: AgentNameStyle) -> Option<String> {
    match style {
        AgentNameStyle::Complete => {
            // "Display Name (username)" when the display name is a real custom
            // name; otherwise the plain legacy / display name.
            match (
                record.display_name.as_deref(),
                record.username.as_deref(),
                record.is_display_name_default,
            ) {
                (Some(display), Some(username), false) => Some(format!("{display} ({username})")),
                _default => record.preferred_name().map(str::to_owned),
            }
        }
        AgentNameStyle::Display => record.preferred_name().map(str::to_owned),
        AgentNameStyle::Username => record
            .username
            .as_deref()
            .or(record.legacy.as_deref())
            .map(str::to_owned),
    }
}

/// Keep every link label in step with the name caches: a full re-resolve when a
/// cache or the locale changed, otherwise only newly-spawned links. Writes only on
/// a real change, so a static block costs nothing.
fn refresh_link_labels(
    avatars: Option<Res<AvatarState>>,
    groups: Option<Res<GroupsModel>>,
    parcels: Option<Res<ParcelNames>>,
    translator: Translator,
    links: Query<Ref<LinkNode>>,
    mut texts: Query<&mut Text>,
) {
    let sweep = avatars.as_ref().is_some_and(|r| r.is_changed())
        || groups.as_ref().is_some_and(|r| r.is_changed())
        || parcels.as_ref().is_some_and(|r| r.is_changed())
        || translator.changed();
    for link in &links {
        if !sweep && !link.is_changed() {
            continue;
        }
        let Ok(mut text) = texts.get_mut(link.label_entity) else {
            continue;
        };
        let wanted = label_text(
            &link.label,
            avatars.as_deref(),
            groups.as_deref(),
            parcels.as_deref(),
            &translator,
        );
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// Request the display name of each freshly-spawned agent / group / parcel link
/// once, when the cache does not already hold it — so a non-member group's name
/// (or an unresolved avatar / parcel) fills the cache instead of showing the
/// placeholder forever. Mirrors [`crate::ui_name_link`]'s request pass.
fn request_link_names(
    changed: Query<&LinkNode, Added<LinkNode>>,
    avatars: Option<Res<AvatarState>>,
    groups: Option<Res<GroupsModel>>,
    parcels: Option<Res<ParcelNames>>,
    mut commands: MessageWriter<SlCommand>,
) {
    let mut agents: Vec<AgentKey> = Vec::new();
    let mut group_ids: Vec<GroupKey> = Vec::new();
    for link in &changed {
        match &link.label {
            LinkLabel::Agent(agent, _style) => {
                let known = avatars
                    .as_ref()
                    .is_some_and(|a| a.name_of(*agent).is_some());
                if !known && !agents.contains(agent) {
                    agents.push(*agent);
                }
            }
            LinkLabel::Group(group) => {
                let known = groups
                    .as_ref()
                    .is_some_and(|g| g.group_name(*group).is_some());
                if !known && !group_ids.contains(group) {
                    group_ids.push(*group);
                }
            }
            // The parcel cache requests its own listing (it owns the dedup).
            LinkLabel::Parcel(parcel) => {
                if let Some(parcels) = parcels.as_ref() {
                    parcels.request(*parcel, &mut commands);
                }
            }
            LinkLabel::Fixed(_) => {}
        }
    }
    if !agents.is_empty() {
        commands.write(SlCommand(Command::RequestAvatarNames(agents)));
    }
    if !group_ids.is_empty() {
        commands.write(SlCommand(Command::RequestGroupNames(group_ids)));
    }
}

// ---------------------------------------------------------------------------
// Leading icons: the bundled white-mask images, tinted to the link colour.
// ---------------------------------------------------------------------------

/// The preloaded link-icon image handles, tinted at render time to each link's
/// colour by its [`ImageNode`].
#[derive(Resource)]
struct LinkIconAssets {
    /// The resident (`Generic_Person`) icon.
    agent: Handle<Image>,
    /// The group (`Generic_Group`) icon.
    group: Handle<Image>,
    /// The location / SLURL (the reference `Hand`) icon.
    location: Handle<Image>,
}

impl LinkIconAssets {
    /// The handle for `icon`, or `None` for [`LinkIcon::None`].
    fn handle(&self, icon: LinkIcon) -> Option<Handle<Image>> {
        match icon {
            LinkIcon::Agent => Some(self.agent.clone()),
            LinkIcon::Group => Some(self.group.clone()),
            LinkIcon::Location => Some(self.location.clone()),
            LinkIcon::None => None,
        }
    }
}

/// Preload the three link icons once, at startup.
fn setup_link_icons(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(LinkIconAssets {
        agent: assets.load(ICON_AGENT_PATH),
        group: assets.load(ICON_GROUP_PATH),
        location: assets.load(ICON_LOCATION_PATH),
    });
}

/// Fill in each freshly-spawned icon image's handle from the preloaded assets,
/// then drop the marker (the spawn site had no `AssetServer`, so the handle is set
/// here on the frame the icon appears).
fn apply_link_icons(
    assets: Option<Res<LinkIconAssets>>,
    mut icons: Query<(Entity, &LinkIconMarker, &mut ImageNode), Added<LinkIconMarker>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else {
        return;
    };
    for (entity, marker, mut image) in &mut icons {
        if let Some(handle) = assets.handle(marker.0) {
            image.image = handle;
        }
        commands.entity(entity).remove::<LinkIconMarker>();
    }
}

// ---------------------------------------------------------------------------
// The hover tooltip: one shared box that follows the cursor.
// ---------------------------------------------------------------------------

/// The currently-hovered link, published by the hover observers and read by
/// [`position_link_tooltip`]. `None` when no link is hovered.
#[derive(Resource, Default)]
struct HoveredLink {
    /// The hovered link's URL + tooltip category, if any.
    link: Option<HoveredLinkInfo>,
}

/// What the tooltip needs about the hovered link.
#[derive(Debug, Clone)]
struct HoveredLinkInfo {
    /// The link's canonical URL.
    url: String,
    /// The Fluent key of the tooltip category line.
    tooltip_key: &'static str,
}

/// The shared hover-tooltip entities: one absolutely-positioned box under the UI
/// root, shown only while a link is hovered.
#[derive(Resource)]
struct LinkTooltipUi {
    /// The tooltip box (toggled visible / hidden).
    box_entity: Entity,
    /// The tooltip's text node (set to the hovered URL).
    text_entity: Entity,
}

/// Spawn the shared tooltip box once, hidden, under the UI root.
fn setup_link_tooltip(mut commands: Commands, root: Res<UiRoot>) {
    let box_entity = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                max_width: Val::Px(520.0),
                ..default()
            },
            BackgroundColor(TOOLTIP_BACKGROUND),
            BorderColor::all(TOOLTIP_BORDER),
            // The tooltip must never eat a click meant for the world / UI beneath.
            Pickable::IGNORE,
            GlobalZIndex(1000),
            Name::new("link-tooltip"),
            ChildOf(root.0),
        ))
        .id();
    let text_entity = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(TOOLTIP_FONT_SIZE),
            TextColor(TOOLTIP_TEXT),
            Pickable::IGNORE,
            ChildOf(box_entity),
        ))
        .id();
    commands.insert_resource(LinkTooltipUi {
        box_entity,
        text_entity,
    });
}

/// Show / hide / position the shared tooltip: when a link is hovered, set its text
/// to the URL and place it near the cursor; otherwise hide it. Writes only on a
/// change of state, except the per-frame position while shown (the cursor moves).
fn position_link_tooltip(
    hovered: Res<HoveredLink>,
    tooltip: Option<Res<LinkTooltipUi>>,
    translator: Translator,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(tooltip) = tooltip else {
        return;
    };
    let Ok(mut box_node) = nodes.get_mut(tooltip.box_entity) else {
        return;
    };
    let Some(info) = hovered.link.as_ref() else {
        if box_node.display != Display::None {
            box_node.display = Display::None;
        }
        return;
    };
    // Follow the cursor; without a cursor position (e.g. off-window) keep hidden.
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        if box_node.display != Display::None {
            box_node.display = Display::None;
        }
        return;
    };
    box_node.display = Display::Flex;
    box_node.left = Val::Px(cursor.x + TOOLTIP_CURSOR_OFFSET.x);
    box_node.top = Val::Px(cursor.y + TOOLTIP_CURSOR_OFFSET.y);
    // Show the link's actual destination URL, under a localised category line, so
    // the user can vet where the link goes before clicking.
    let wanted = format!("{}\n{}", translator.get(info.tooltip_key), info.url);
    if let Ok(mut text) = texts.get_mut(tooltip.text_entity)
        && text.0 != wanted
    {
        text.0 = wanted;
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// Wires the linkified-text systems: label resolution, name requests, web-link
/// dispatch and the shared hover tooltip. Link click / hover observers are
/// attached per node at [`spawn_link_run`], so a consumer only needs this plugin
/// once.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LinkifiedTextPlugin;

impl Plugin for LinkifiedTextPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LinkActivated>()
            .init_resource::<HoveredLink>()
            .add_systems(
                Startup,
                (
                    setup_link_tooltip.after(crate::ui::UiScaffoldSystems::SpawnRoot),
                    setup_link_icons,
                ),
            )
            .add_systems(
                Update,
                (
                    refresh_link_labels,
                    request_link_names,
                    apply_link_icons,
                    dispatch_web_links,
                    position_link_tooltip,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The widest a specimen block grows before its plain runs wrap.
const SPECIMEN_MAX_WIDTH: f32 = 420.0;

/// The gallery / `ui_test` specimen: a paragraph mixing plain prose with a web
/// URL, a labelled link and a location SLURL, so the segment rendering / wrapping
/// / link tint is swept login-free across every script, translation length and UI
/// scale. Registered in [`crate::ui_element::ELEMENTS`].
///
/// The connective prose runs through the cell's string transform (so the matrix
/// sweeps translations), but the URLs stay native — a mangled URL would not
/// linkify, which is not what this specimen is testing. A URL is a single
/// unbreakable node, so the subtree is declared [`TextMayClip`].
pub(crate) fn spawn_linkified_text_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                max_width: Val::Px(SPECIMEN_MAX_WIDTH),
                ..default()
            },
            TextMayClip {
                reason: "a URL is a single unbreakable link node and may exceed the block width",
            },
            Name::new("linkified-text-specimen"),
            ChildOf(parent),
        ))
        .id();
    let sample = format!(
        "{see} https://example.com/page — {or} [secondlife://Morris/128/24 {my_spot}]",
        see = cx.text("See"),
        or = cx.text("or visit"),
        my_spot = cx.text("my spot"),
    );
    spawn_linkified_text(commands, root, &sample, LinkTextStyle::at(cx.font_size));
    root
}

#[cfg(test)]
mod tests {
    use super::{LinkTextStyle, agent_label, spawn_linkified_text};
    use crate::avatars::NameRecord;
    use crate::ui::{UiRoot, UiScaffoldSystems};
    use crate::ui_test::{LayoutTest, TestError, settle};
    use crate::url_linkify::AgentNameStyle;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A complete-style label is "Display (username)" for a custom display name,
    /// and the plain preferred name for a default one.
    #[test]
    fn agent_complete_label_forms() {
        let custom = NameRecord {
            legacy: Some("First Last".to_owned()),
            username: Some("first.last".to_owned()),
            display_name: Some("Coolname".to_owned()),
            is_display_name_default: false,
        };
        assert_eq!(
            agent_label(&custom, AgentNameStyle::Complete),
            Some("Coolname (first.last)".to_owned())
        );
        assert_eq!(
            agent_label(&custom, AgentNameStyle::Username),
            Some("first.last".to_owned())
        );
        assert_eq!(
            agent_label(&custom, AgentNameStyle::Display),
            Some("Coolname".to_owned())
        );

        // A default display name shows the plain preferred name for Complete.
        let default = NameRecord {
            legacy: Some("First Last".to_owned()),
            username: Some("first.last".to_owned()),
            display_name: Some("First Last".to_owned()),
            is_display_name_default: true,
        };
        assert_eq!(
            agent_label(&default, AgentNameStyle::Complete),
            Some("First Last".to_owned())
        );
    }

    /// Spawning a linkified block with a plain URL produces a wrapping container
    /// with exactly one clickable link child, whose target and URL are the parsed
    /// web link.
    #[test]
    fn spawn_builds_a_link_child_for_a_url() -> Result<(), TestError> {
        use super::LinkNode;
        use crate::url_linkify::LinkTarget;

        let mut app = LayoutTest::new().build();
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<UiRoot>| {
                spawn_linkified_text(
                    &mut commands,
                    root.0,
                    "see https://example.com now",
                    LinkTextStyle::default(),
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);

        let mut query = app.world_mut().query::<&LinkNode>();
        let links: Vec<&LinkNode> = query.iter(app.world()).collect();
        assert_eq!(links.len(), 1, "expected exactly one link node");
        let link = links.first().ok_or("no link node")?;
        assert_eq!(link.url, "https://example.com");
        assert_eq!(link.target, LinkTarget::Web { trusted: false });
        Ok(())
    }
}
