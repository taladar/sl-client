//! One node of a semantic UI snapshot, and the roles, states, values, bounds
//! and visibility it carries.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// What a UI node *is* to its user — the accessibility role, which is also
/// the first thing a [`Locator`](crate::Locator) names.
///
/// Serialized in lowercase (`"menuitem"`, `"listitem"`), the spelling ARIA
/// uses for the same roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A push button.
    Button,
    /// A two-state (or mixed) check box.
    Checkbox,
    /// One option of a radio group.
    Radio,
    /// A set of radio options, one focus stop; its options are its children.
    RadioGroup,
    /// A single- or multi-line text field.
    Textbox,
    /// A drop-down: a closed control that opens a list of choices.
    Combobox,
    /// A slider over a numeric range.
    Slider,
    /// A number field with a pair of step arrows beside it: typed into like a
    /// text field, its value is the number it holds, and its arrows — two
    /// buttons beside it in the spinner's group — step that number up and
    /// down.
    SpinButton,
    /// A swatch showing a colour, which opens a colour picker.
    ColorWell,
    /// A ball dragged to aim a direction or a rotation.
    Trackball,
    /// The row of tabs of a tab container; its tabs are its children.
    TabList,
    /// One tab of a tab container.
    Tab,
    /// A bar of menu buttons along the top of a window.
    MenuBar,
    /// An open menu: a drop-down, a context menu or a pie menu; its entries
    /// are its children.
    Menu,
    /// One entry of a menu bar, a menu, a context menu or a pie menu.
    MenuItem,
    /// A list or a grid; its rows are its children.
    List,
    /// One row of a list or a grid.
    ListItem,
    /// A tree; its rows are its children, their depth their
    /// [`level`](UiNode::level).
    Tree,
    /// One row of a tree.
    TreeItem,
    /// A floater or another top-level window.
    Window,
    /// Static text.
    Text,
    /// An image with no interaction of its own.
    Image,
    /// A web page the viewer shows: named by the page's title, its value the
    /// address it is at.
    Document,
    /// A container with no widget role of its own — a panel, a list body, a
    /// scroll area — kept in the tree so a locator can scope to it.
    Group,
}

impl Role {
    /// Every role, in declaration order.
    pub const ALL: [Self; 24] = [
        Self::Button,
        Self::Checkbox,
        Self::Radio,
        Self::RadioGroup,
        Self::Textbox,
        Self::Combobox,
        Self::Slider,
        Self::SpinButton,
        Self::ColorWell,
        Self::Trackball,
        Self::TabList,
        Self::Tab,
        Self::MenuBar,
        Self::Menu,
        Self::MenuItem,
        Self::List,
        Self::ListItem,
        Self::Tree,
        Self::TreeItem,
        Self::Window,
        Self::Text,
        Self::Image,
        Self::Document,
        Self::Group,
    ];

    /// The role's serialized spelling, used for display too.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::RadioGroup => "radiogroup",
            Self::Textbox => "textbox",
            Self::Combobox => "combobox",
            Self::Slider => "slider",
            Self::SpinButton => "spinbutton",
            Self::ColorWell => "colorwell",
            Self::Trackball => "trackball",
            Self::TabList => "tablist",
            Self::Tab => "tab",
            Self::MenuBar => "menubar",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Tree => "tree",
            Self::TreeItem => "treeitem",
            Self::Window => "window",
            Self::Text => "text",
            Self::Image => "image",
            Self::Document => "document",
            Self::Group => "group",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A boolean state a node may be in; a node's states are the set of those it
/// is in, and every state it is absent from is false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Disabled, on the node itself **or any ancestor** — the viewer's
    /// disabled marker is advisory, so the snapshot is what makes an
    /// inherited one observable.
    Disabled,
    /// A text field that shows its value but refuses edits.
    ReadOnly,
    /// A checked check box, a chosen radio option, a ticked menu entry.
    Checked,
    /// A selected list row, the front tab of a tab container.
    Selected,
    /// An open combo box, an unfolded tree row, an open menu.
    Expanded,
    /// Holds the keyboard focus.
    Focused,
    /// Under the pointer.
    Hovered,
}

/// The value a node shows, for the roles that have one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeValue {
    /// The text of a text field, or the chosen entry of a combo box.
    Text(String),
    /// The position of a slider or a spinner.
    Number(f32),
    /// The colour a colour well holds, as `#rrggbb` in sRGB — `#rrggbbaa`
    /// when it is not opaque.
    Color(String),
}

impl NodeValue {
    /// Whether a field filled with `text` now holds it: the same text, or — a
    /// spin button being valued by its number — the number `text` spells.
    #[must_use]
    pub fn holds(&self, text: &str) -> bool {
        match self {
            Self::Text(held) | Self::Color(held) => held == text,
            Self::Number(number) => text
                .trim()
                .parse::<f32>()
                .is_ok_and(|typed| typed.to_bits() == number.to_bits()),
        }
    }
}

/// Whether, and why not, a node can be seen.
///
/// Only [`Visible`](Self::Visible) is actionable; the others name the first
/// reason a user could not see the node, checked in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeVisibility {
    /// Drawn, on screen and not covered.
    Visible,
    /// Not drawn at all: hidden itself, or laid out away (display none) on
    /// the node or an ancestor.
    Hidden,
    /// Scrolled out of the visible part of an enclosing scroll area.
    Clipped,
    /// Outside the viewport.
    OffScreen,
    /// Drawn, but something else is on top of it: a hit test at its centre
    /// lands on a node that is neither it nor one of its descendants.
    Covered,
}

/// A node's box in **logical** pixels, the origin at the viewport's top left
/// and `y` growing downwards.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Bounds {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub width: f32,
    /// The height.
    pub height: f32,
}

/// An opaque handle on one node of one viewer, stable for as long as that
/// node exists and meaningless to any other viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u64);

/// One node of a semantic UI snapshot.
///
/// The optional parts are left out of the JSON when absent, so a snapshot of
/// a large floater stays readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiNode {
    /// The node's handle within its viewer.
    pub id: NodeId,
    /// What the node is.
    pub role: Role,
    /// The accessible name as the user reads it, resolved in the viewer's
    /// current locale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The Fluent key the name was translated from, when it was — the
    /// locale-independent way to address the node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_key: Option<String>,
    /// The viewer's own identifier for the node (in sl-client, the entity's
    /// `Name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    /// The states the node is in.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub states: BTreeSet<NodeState>,
    /// The value the node shows, for text fields, combo boxes, sliders and
    /// colour wells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<NodeValue>,
    /// The colour a text node is drawn in, as `#rrggbb` in sRGB —
    /// `#rrggbbaa` when it is not opaque. What a skin or a colour preference
    /// changes; absent for every other role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// How deep a tree row sits, the top level being 1 (as ARIA counts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    /// The keyboard shortcut a menu entry shows, as the user reads it
    /// (`Ctrl+B`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accelerator: Option<String>,
    /// The node's box.
    pub bounds: Bounds,
    /// Whether the node can be seen.
    pub visibility: NodeVisibility,
    /// The node's children, in the order the user reads them. Empty for a
    /// leaf, and for a node reported on its own rather than in a tree.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Self>,
}

impl UiNode {
    /// Whether the node is in `state`.
    #[must_use]
    pub fn has_state(&self, state: NodeState) -> bool {
        self.states.contains(&state)
    }
}

/// One line naming the node for a person reading an error — role, name, key,
/// test id and box: `button "OK" key=button-ok #prefs.ok at 10,20 120x30`.
/// Its states and children are left out.
impl fmt::Display for UiNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.role.as_str())?;
        if let Some(name) = &self.name {
            write!(f, " {name:?}")?;
        }
        if let Some(key) = &self.name_key {
            write!(f, " key={key}")?;
        }
        if let Some(id) = &self.test_id {
            write!(f, " #{id}")?;
        }
        let Bounds {
            x,
            y,
            width,
            height,
        } = self.bounds;
        write!(f, " at {x},{y} {width}x{height}")
    }
}
