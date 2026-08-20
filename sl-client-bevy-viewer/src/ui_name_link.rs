//! A **reusable clickable name-link widget** (`viewer-clickable-name-widgets`).
//!
//! Clickable avatar and group *names* appear all over the viewer UI — parcel and
//! object owners, region / estate / covenant owners, chat senders, member and
//! friend lists. Every site re-implemented the same two concerns:
//!
//! 1. **Name resolution** — look an id up in [`AvatarState::name_of`] /
//!    [`GroupsModel::group_name`], fall back to `(id)` until the cache resolves,
//!    request the name once, and re-resolve in place when the cache changes.
//! 2. **Click → profile** — open [`OpenAvatarProfile`] / [`OpenGroupProfile`] on
//!    press, tinting the label as a link only when it actually points somewhere.
//!
//! This module owns both, once, behind one **owner-kind-aware** widget. Its
//! binding is an [`OwnerKey`] (an agent *or* a group), so the avatar-only and
//! group-only cases are just the two concrete key kinds fed through a single
//! resolution + click path; a caller binds a [`NameTarget<AgentKey>`],
//! [`NameTarget<GroupKey>`], or [`NameTarget<OwnerKey>`] and the widget does the
//! rest.
//!
//! # Three display states, not two
//!
//! "We do not know the owner yet" (the reply is still in flight) is distinct from
//! "there is genuinely no owner", so the binding is a **tri-state**
//! ([`NameTarget`] / [`NameBinding`]) rather than a bare `Option`:
//!
//! - **Loading** — data not yet received: the configured loading label
//!   (e.g. `(loading)`), plain colour, non-clickable.
//! - **Unset** — known to have no owner: the configured unset label
//!   (e.g. `(none)`), plain colour, non-clickable.
//! - **Set(key)** — a real owner: the resolved name (or `(id)` until the name
//!   cache resolves), tinted as a clickable link, opening the right profile.
//!
//! Both non-clickable labels are Fluent keys chosen per call site, so the widget
//! relocalises them on a locale switch like any other UI text.
//!
//! Reference (Firestorm, read-only): `LLNameEditor` / `LLAvatarName` and the
//! common "click a resident / group name → profile" behaviour.

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, Command, GroupKey, OwnerKey, SlCommand};

use crate::avatar_profile::OpenAvatarProfile;
use crate::avatars::AvatarState;
use crate::group_profile::OpenGroupProfile;
use crate::groups::GroupsModel;
use crate::i18n::Translator;
use crate::ui_font::UiFont;

/// The default link tint — the same cornflower blue the bespoke owner links used,
/// so migrated sites look unchanged.
pub(crate) const NAME_LINK_COLOR: Color = Color::srgb(0.52, 0.68, 0.95);

/// The default plain (non-link) colour, matching the floaters' value labels.
pub(crate) const NAME_PLAIN_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The default label font size, in logical pixels — the floater value size.
const DEFAULT_FONT_SIZE: f32 = 13.0;

// ---------------------------------------------------------------------------
// The binding: an owner key, or one of the two "no key" states.
// ---------------------------------------------------------------------------

/// The stored tri-state of a [`NameLink`]: not-yet-known, known-to-be-absent, or
/// a real owner. Kept owner-kind-aware ([`OwnerKey`]) so one resolution + click
/// path serves avatars and groups alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameBinding {
    /// The owner is not yet known (a reply is still in flight).
    Loading,
    /// The owner is known to be absent.
    Unset,
    /// A real owner — an agent or a group.
    Set(OwnerKey),
}

/// A caller-facing tri-state binding for a [`NameLink`], generic over the key
/// kind so an avatar-only site binds a `NameTarget<AgentKey>`, a group-only site
/// a `NameTarget<GroupKey>`, and an owner site a `NameTarget<OwnerKey>`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NameTarget<K> {
    /// The value is not yet available — show the loading label.
    Loading,
    /// The value is known to be absent — show the unset label.
    Unset,
    /// A real key — resolve, tint as a link, open its profile on click.
    Set(K),
}

impl<K> NameTarget<K> {
    /// Build a target from an `Option` plus whether the underlying data has
    /// arrived: `Loading` before it has, then `Unset` / `Set` once it has. This
    /// is the exact "reply present? owner present?" shape the floaters carry.
    pub(crate) fn from_option(loaded: bool, value: Option<K>) -> Self {
        match (loaded, value) {
            (false, _absent_or_present) => Self::Loading,
            (true, None) => Self::Unset,
            (true, Some(key)) => Self::Set(key),
        }
    }
}

impl<K: IntoOwnerKey> NameTarget<K> {
    /// Lower a caller target into the stored [`NameBinding`].
    fn into_binding(self) -> NameBinding {
        match self {
            Self::Loading => NameBinding::Loading,
            Self::Unset => NameBinding::Unset,
            Self::Set(key) => NameBinding::Set(key.into_owner_key()),
        }
    }
}

/// A key kind that can be viewed as an [`OwnerKey`], so the one widget serves
/// avatar-only, group-only, and owner (either-kind) bindings through a single
/// resolution + click path.
pub(crate) trait IntoOwnerKey {
    /// This key as an owner key.
    fn into_owner_key(self) -> OwnerKey;
}

impl IntoOwnerKey for AgentKey {
    fn into_owner_key(self) -> OwnerKey {
        OwnerKey::Agent(self)
    }
}

impl IntoOwnerKey for GroupKey {
    fn into_owner_key(self) -> OwnerKey {
        OwnerKey::Group(self)
    }
}

impl IntoOwnerKey for OwnerKey {
    fn into_owner_key(self) -> OwnerKey {
        self
    }
}

// ---------------------------------------------------------------------------
// The spawn spec and the component.
// ---------------------------------------------------------------------------

/// How a [`NameLink`] node looks: its two non-clickable labels (Fluent keys), an
/// optional group-owned annotation, its font size, and its two colours.
#[derive(Debug, Clone)]
pub(crate) struct NameLinkSpec {
    /// Fluent key for the **loading** label (data not yet received).
    loading_key: String,
    /// Fluent key for the **unset** label (known to have no owner).
    unset_key: String,
    /// Optional Fluent key appended (after a space) when the resolved owner is a
    /// **group** — About Land's parcel-owner field annotates a deeded parcel with
    /// "(group owned)". `None` (the default) never annotates, which is what a
    /// plain group-name field wants.
    group_suffix_key: Option<String>,
    /// The label font size, in logical pixels.
    font_size: f32,
    /// The colour of a live, clickable link (a `Set` binding).
    link_color: Color,
    /// The colour of a non-clickable label (a `Loading` / `Unset` binding).
    plain_color: Color,
}

impl NameLinkSpec {
    /// A spec with the two non-clickable labels and the default font / colours.
    pub(crate) fn new(loading_key: impl Into<String>, unset_key: impl Into<String>) -> Self {
        Self {
            loading_key: loading_key.into(),
            unset_key: unset_key.into(),
            group_suffix_key: None,
            font_size: DEFAULT_FONT_SIZE,
            link_color: NAME_LINK_COLOR,
            plain_color: NAME_PLAIN_COLOR,
        }
    }

    /// Annotate a group owner with `key`'s text (after a space) — the parcel
    /// owner field's "(group owned)" suffix.
    #[must_use]
    pub(crate) fn with_group_suffix(mut self, key: impl Into<String>) -> Self {
        self.group_suffix_key = Some(key.into());
        self
    }
}

/// A clickable name node: it carries the tri-state binding and the display
/// config, its text and colour are kept in step with the name cache by
/// [`refresh_name_links`], and pressing it opens the bound owner's profile.
#[derive(Component, Debug, Clone)]
pub(crate) struct NameLink {
    /// The current tri-state binding.
    binding: NameBinding,
    /// Fluent key for the loading label.
    loading_key: String,
    /// Fluent key for the unset label.
    unset_key: String,
    /// Fluent key appended for a group owner, if any.
    group_suffix_key: Option<String>,
    /// The link tint.
    link_color: Color,
    /// The plain (non-link) colour.
    plain_color: Color,
}

// ---------------------------------------------------------------------------
// Spawn / bind.
// ---------------------------------------------------------------------------

/// Spawn a name-link node under `parent`, starting in the [`Loading`] state. The
/// returned entity is the handle a caller binds with [`set_name_link`].
///
/// [`Loading`]: NameBinding::Loading
pub(crate) fn spawn_name_link(
    commands: &mut Commands,
    parent: Entity,
    spec: NameLinkSpec,
) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            Button,
            NameLink {
                binding: NameBinding::Loading,
                loading_key: spec.loading_key,
                unset_key: spec.unset_key,
                group_suffix_key: spec.group_suffix_key,
                link_color: spec.link_color,
                plain_color: spec.plain_color,
            },
            UiFont::Sans.at(spec.font_size),
            TextColor(spec.plain_color),
            Pickable::default(),
            ChildOf(parent),
        ))
        .observe(on_name_link_press)
        .id()
}

/// Bind `node`'s name link to `target`, changing the stored binding only when it
/// differs (so [`request_name_links`] fires a resolve request exactly once per
/// real change, and the text / colour sweep stays cheap). A `None` node is a
/// no-op, matching the floaters' `Option<Entity>` handles.
pub(crate) fn set_name_link<K: IntoOwnerKey>(
    links: &mut Query<&mut NameLink>,
    node: Option<Entity>,
    target: NameTarget<K>,
) {
    let Some(node) = node else {
        return;
    };
    let wanted = target.into_binding();
    if let Ok(mut link) = links.get_mut(node)
        && link.binding != wanted
    {
        link.binding = wanted;
    }
}

// ---------------------------------------------------------------------------
// Systems.
// ---------------------------------------------------------------------------

/// The label text and whether it is a live link, for a binding whose name (for a
/// `Set` binding) has already been looked up. Pure so the tri-state / `(id)`
/// fallback / group-suffix rules are unit-testable without an ECS world.
fn display(
    binding: NameBinding,
    resolved: Option<&str>,
    loading_label: &str,
    unset_label: &str,
    group_suffix: Option<&str>,
) -> (String, bool) {
    match binding {
        NameBinding::Loading => (loading_label.to_owned(), false),
        NameBinding::Unset => (unset_label.to_owned(), false),
        NameBinding::Set(owner) => {
            let base = resolved.map_or_else(|| format!("({})", owner.uuid()), str::to_owned);
            let text = match (group_suffix, owner) {
                (Some(suffix), OwnerKey::Group(_group)) => format!("{base} {suffix}"),
                (_no_suffix_or_agent, _owner) => base,
            };
            (text, true)
        }
    }
}

/// Keep every [`NameLink`]'s text and colour in step with the name caches: a full
/// re-resolve when a cache or the locale changed, otherwise only links whose
/// binding changed this frame. Writes only on a real change, so a static link
/// costs nothing.
fn refresh_name_links(
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    translator: Translator,
    mut links: Query<(Ref<NameLink>, &mut Text, &mut TextColor)>,
) {
    let sweep = avatars.is_changed() || groups.is_changed() || translator.changed();
    for (link, mut text, mut color) in &mut links {
        if !sweep && !link.is_changed() {
            continue;
        }
        let resolved = match link.binding {
            NameBinding::Set(OwnerKey::Agent(agent)) => {
                avatars.shown_name_of(agent).map(str::to_owned)
            }
            NameBinding::Set(OwnerKey::Group(group)) => groups.group_name(group).map(str::to_owned),
            NameBinding::Loading | NameBinding::Unset => None,
        };
        let loading = translator.get(&link.loading_key);
        let unset = translator.get(&link.unset_key);
        let suffix = link
            .group_suffix_key
            .as_ref()
            .map(|key| translator.get(key));
        let (label, is_link) = display(
            link.binding,
            resolved.as_deref(),
            &loading,
            &unset,
            suffix.as_deref(),
        );
        if text.0 != label {
            text.0 = label;
        }
        let wanted = TextColor(if is_link {
            link.link_color
        } else {
            link.plain_color
        });
        if *color != wanted {
            *color = wanted;
        }
    }
}

/// Request the display name of a freshly-bound `Set` link once — at the discrete
/// bind event ([`Changed`]), and only when the cache does not already hold it, so
/// a non-member group's name (or an unresolved avatar's) fills the cache instead
/// of showing a raw id forever.
fn request_name_links(
    changed: Query<&NameLink, Changed<NameLink>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    for link in &changed {
        match link.binding {
            NameBinding::Set(OwnerKey::Agent(agent)) => {
                if avatars.name_of(agent).is_none() {
                    commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
                }
            }
            NameBinding::Set(OwnerKey::Group(group)) => {
                if groups.group_name(group).is_none() {
                    commands.write(SlCommand(Command::RequestGroupNames(vec![group])));
                }
            }
            NameBinding::Loading | NameBinding::Unset => {}
        }
    }
}

/// Open the bound owner's profile on a primary press — an avatar or a group
/// profile by kind; a `Loading` / `Unset` link does nothing.
fn on_name_link_press(
    press: On<Pointer<Press>>,
    links: Query<&NameLink>,
    mut avatar_profiles: MessageWriter<OpenAvatarProfile>,
    mut group_profiles: MessageWriter<OpenGroupProfile>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(link) = links.get(press.entity) else {
        return;
    };
    match link.binding {
        NameBinding::Set(OwnerKey::Agent(agent)) => {
            avatar_profiles.write(OpenAvatarProfile { agent });
        }
        NameBinding::Set(OwnerKey::Group(group)) => {
            group_profiles.write(OpenGroupProfile { group });
        }
        NameBinding::Loading | NameBinding::Unset => {}
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// Wires the name-link resolve / request systems. The click observer is attached
/// per node at [`spawn_name_link`], so a consumer only needs this plugin once.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NameLinkPlugin;

impl Plugin for NameLinkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (refresh_name_links, request_name_links));
    }
}

#[cfg(test)]
mod tests {
    use super::IntoOwnerKey as _;
    use super::{NameBinding, NameTarget, display};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, GroupKey, OwnerKey, Uuid};

    /// A stable agent key for the tests.
    fn agent() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0x1234))
    }

    /// A stable group key for the tests.
    fn group() -> GroupKey {
        GroupKey::from(Uuid::from_u128(0x5678))
    }

    #[test]
    fn from_option_maps_the_three_states() {
        // Not loaded yet → Loading, regardless of the value.
        assert!(matches!(
            NameTarget::from_option(false, Some(agent())),
            NameTarget::Loading
        ));
        assert!(matches!(
            NameTarget::<AgentKey>::from_option(false, None),
            NameTarget::Loading
        ));
        // Loaded, no value → Unset.
        assert!(matches!(
            NameTarget::<AgentKey>::from_option(true, None),
            NameTarget::Unset
        ));
        // Loaded with a value → Set.
        assert!(matches!(
            NameTarget::from_option(true, Some(agent())),
            NameTarget::Set(_key)
        ));
    }

    #[test]
    fn into_owner_key_covers_all_three_kinds() {
        assert_eq!(agent().into_owner_key(), OwnerKey::Agent(agent()));
        assert_eq!(group().into_owner_key(), OwnerKey::Group(group()));
        let owner = OwnerKey::Group(group());
        assert_eq!(owner.into_owner_key(), owner);
    }

    #[test]
    fn loading_and_unset_are_plain_labels() {
        assert_eq!(
            display(NameBinding::Loading, None, "(loading)", "(none)", None),
            ("(loading)".to_owned(), false)
        );
        assert_eq!(
            display(NameBinding::Unset, None, "(loading)", "(none)", None),
            ("(none)".to_owned(), false)
        );
    }

    #[test]
    fn set_resolves_to_the_name_and_is_a_link() {
        let binding = NameBinding::Set(OwnerKey::Agent(agent()));
        assert_eq!(
            display(binding, Some("Alice Resident"), "(loading)", "(none)", None),
            ("Alice Resident".to_owned(), true)
        );
    }

    #[test]
    fn set_falls_back_to_the_id_until_resolved() {
        let binding = NameBinding::Set(OwnerKey::Agent(agent()));
        let (label, is_link) = display(binding, None, "(loading)", "(none)", None);
        assert!(is_link);
        assert_eq!(label, format!("({})", agent().into_owner_key().uuid()));
    }

    #[test]
    fn group_suffix_annotates_only_a_group_owner() {
        // A group owner gets the suffix.
        let group_owner = NameBinding::Set(OwnerKey::Group(group()));
        assert_eq!(
            display(
                group_owner,
                Some("The Group"),
                "(loading)",
                "(none)",
                Some("(group owned)")
            ),
            ("The Group (group owned)".to_owned(), true)
        );
        // An agent owner never gets it, even when a suffix is configured.
        let agent_owner = NameBinding::Set(OwnerKey::Agent(agent()));
        assert_eq!(
            display(
                agent_owner,
                Some("Alice Resident"),
                "(loading)",
                "(none)",
                Some("(group owned)")
            ),
            ("Alice Resident".to_owned(), true)
        );
    }
}
