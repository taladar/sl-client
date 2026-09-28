//! **Semantic roles** for the widgets `bevy_ui` does not know.
//!
//! The automation model (`sl-viewer-automation`) infers a role from the stock
//! widget components — a `Button`, a `Checkbox`, an `EditableText` — but most of
//! the viewer is widgets built out of plain nodes: floaters, tab rows, virtual
//! lists, the inventory tree, menus, combos, the pie menu, the colour well, the
//! trackball. Without being told, the model sees a pile of `Text` under them.
//!
//! A widget says what it is by putting a [`Semantic`] on its root when it is
//! spawned: the [`Role`], where its accessible name comes from, and — for the
//! roles that have them — its tree level, its accelerator and the node showing
//! its value. A `Semantic` wins over everything the model would infer.
//!
//! The states are markers, so a skin rule and the model read the same thing:
//!
//! - **selected**: `bevy_ui`'s `Selected`, or [`SELECTED_CLASS`] on a recycled
//!   row (whose selection belongs to its data index, which no engine state
//!   tracks), or `Checked` on a [`Role::Tab`] — the front tab of a strip is the
//!   strip's checked radio option;
//! - **expanded**: [`Expanded`], which the widget keeps in step with its own
//!   open state (an open combo or menu, an unfolded tree row).
//!
//! A row the skin draws as a list row ([`LIST_ROW_CLASS`], [`TABLE_ROW_CLASS`],
//! [`COMBO_OPTION_CLASS`]) is a [`Role::ListItem`] without saying so; see
//! [`role_from_classes`].

use std::borrow::Cow;

use bevy::prelude::*;
use bevy_flair::prelude::ClassList;
pub use sl_automation_proto::Role;

use crate::skin::{COMBO_OPTION_CLASS, LIST_ROW_CLASS, SELECTED_CLASS, TABLE_ROW_CLASS};

/// Where a node's accessible name comes from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SemanticName {
    /// From what the node shows — its role's default: a control is named by
    /// its label text, a container by nothing.
    #[default]
    Content,
    /// From another node's text — a floater by its title, a tab by its
    /// caption, a menu entry by its label without the check mark and the
    /// accelerator beside it. The Fluent key of that text, when it has one,
    /// is reported too.
    LabelledBy(Entity),
    /// From a Fluent key, resolved in the viewer's locale — for a node no
    /// visible text names (an icon, a trackball, a menu entry whose label is
    /// resolved when the menu is built).
    Key(Cow<'static, str>),
}

/// What a custom widget is to its user: its role, where its name comes from,
/// and the extra facts some roles carry.
///
/// Put on the widget's **root** at spawn. It overrides the role the automation
/// model would infer, so a stock `Button` that is really a menu entry, or a
/// radio option that is really a tab, reads as what it is.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Semantic {
    /// What the node is.
    role: Role,
    /// Where its accessible name comes from.
    name: SemanticName,
    /// How deep a tree row sits, the top level being 1.
    level: Option<u32>,
    /// The shortcut a menu entry shows.
    accelerator: Option<Cow<'static, str>>,
    /// The node whose text is the widget's value — a combo's chosen entry.
    value_from: Option<Entity>,
}

impl Semantic {
    /// A node of `role`, named by its content.
    #[must_use]
    pub const fn new(role: Role) -> Self {
        Self {
            role,
            name: SemanticName::Content,
            level: None,
            accelerator: None,
            value_from: None,
        }
    }

    /// Named by `label`'s text (and its Fluent key, when it is translated).
    #[must_use]
    pub fn labelled_by(mut self, label: Entity) -> Self {
        self.name = SemanticName::LabelledBy(label);
        self
    }

    /// Named by the Fluent key `key`, resolved in the viewer's locale.
    #[must_use]
    pub fn name_key(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.name = SemanticName::Key(key.into());
        self
    }

    /// A tree row `level` deep, the top level being 1.
    #[must_use]
    pub const fn level(mut self, level: u32) -> Self {
        self.level = Some(level);
        self
    }

    /// A menu entry showing the shortcut `accelerator`.
    #[must_use]
    pub fn accelerator(mut self, accelerator: impl Into<Cow<'static, str>>) -> Self {
        self.accelerator = Some(accelerator.into());
        self
    }

    /// Whose text is the widget's value.
    #[must_use]
    pub const fn value_from(mut self, value: Entity) -> Self {
        self.value_from = Some(value);
        self
    }

    /// What the node is.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// Where its accessible name comes from.
    #[must_use]
    pub const fn name(&self) -> &SemanticName {
        &self.name
    }

    /// How deep a tree row sits, when it is one.
    #[must_use]
    pub const fn tree_level(&self) -> Option<u32> {
        self.level
    }

    /// The shortcut a menu entry shows, when it shows one.
    #[must_use]
    pub fn shortcut(&self) -> Option<&str> {
        self.accelerator.as_deref()
    }

    /// The node whose text is the widget's value, when it has one.
    #[must_use]
    pub const fn value_node(&self) -> Option<Entity> {
        self.value_from
    }
}

/// A label naming this node, or the controls inside it — the `<label>` of a
/// form row, for a field, a slider or a swatch that draws no text of its own.
///
/// Put on a whole row by
/// [`spawn_labeled_row`](crate::ui_spawn::spawn_labeled_row), or on one
/// control whose caption sits elsewhere. The automation model falls back to it
/// — on the control, else its nearest ancestor that has one — for a control
/// that names nothing itself.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelledBy(pub Entity);

/// An open combo box, an open menu, an unfolded tree row.
///
/// The widget that owns the open state keeps this in step with it, through
/// [`sync_expanded`], so the model never has to know each widget's own flag.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Expanded;

/// Put [`Expanded`] on `entity`, or take it off, so it says `wanted`.
///
/// `current` is whether it carries the marker now (a `Has<Expanded>` the
/// caller already read): nothing is queued when the two agree, so a widget
/// whose open state did not change is not marked changed.
pub fn sync_expanded(commands: &mut Commands, entity: Entity, wanted: bool, current: bool) {
    if wanted == current {
        return;
    }
    if let Ok(mut target) = commands.get_entity(entity) {
        if wanted {
            target.insert(Expanded);
        } else {
            target.remove::<Expanded>();
        }
    }
}

/// The role a node's skin classes give it, when it has no [`Semantic`] and
/// no widget component: a row drawn as a list row is a list item.
///
/// Read from the classes because the hand-rolled lists — dozens of them —
/// share the row classes and nothing else, and the class is what makes a row
/// look like one.
#[must_use]
pub fn role_from_classes(classes: &ClassList) -> Option<Role> {
    [LIST_ROW_CLASS, TABLE_ROW_CLASS, COMBO_OPTION_CLASS]
        .iter()
        .any(|class| classes.contains(*class))
        .then_some(Role::ListItem)
}

/// Whether a node's skin classes draw it selected.
#[must_use]
pub fn selected_by_class(classes: &ClassList) -> bool {
    classes.contains(SELECTED_CLASS)
}
