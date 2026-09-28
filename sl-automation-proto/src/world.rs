//! World locators: the semantic query that names which in-world thing — an
//! object, an avatar, the own avatar, a worn attachment — a test means, and
//! the [`WorldNode`] a viewer reports for one.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::locator::NameMatcher;

/// What an in-world thing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldKind {
    /// A resident's avatar, the own one included.
    Avatar,
    /// A prim, mesh, tree or grass object that nobody wears — a linkset root
    /// or one of its child prims.
    Object,
    /// An object worn by an avatar: the attachment's root prim or one of its
    /// child prims.
    Attachment,
}

impl WorldKind {
    /// The kind's serialized spelling, used for display too.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Avatar => "avatar",
            Self::Object => "object",
            Self::Attachment => "attachment",
        }
    }
}

impl fmt::Display for WorldKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a proximity criterion measures distances from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// A point in region-local metres (`[x, y, z]`, Second Life axes: `x`
    /// east, `y` north, `z` up) of the agent's current region.
    Point([f32; 3]),
    /// The own avatar, wherever it is when the locator is resolved.
    OwnAvatar,
}

/// A proximity criterion: distances measured from an [`Anchor`], optionally
/// only up to a radius.
///
/// A locator with one orders its matches nearest first, so
/// [`WorldLocator::nth`] counts outwards from the anchor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Near {
    /// What distances are measured from.
    pub to: Anchor,
    /// Only things at most this many metres away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
}

/// A semantic query for in-world things: a kind plus a name, an id, an owner,
/// an object class or floating text, narrowed by proximity and picked with
/// [`nth`](Self::nth).
///
/// Every criterion that is set must hold; an unset one matches anything, so
/// [`WorldLocator::default`] matches everything the viewer tracks. As with a
/// UI locator, an action on a world locator that matches more than one thing
/// is refused as ambiguous rather than applied to the first.
///
/// Names and owners of objects are not part of the stream that places them —
/// the viewer asks the simulator for them — so a locator naming either may
/// have to wait for replies before it can say what matches.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldLocator {
    /// What the thing is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<WorldKind>,
    /// The own avatar and what it wears (`true`), or everything else
    /// (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<bool>,
    /// A comparison against the name: an object's name, an avatar's shown
    /// name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<NameMatcher>,
    /// The grid-wide id: an object's full id, an avatar's agent id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_id: Option<Uuid>,
    /// The region-local id the simulator streams the thing under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<u32>,
    /// The owner's id — an agent, or a group for a deeded object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Uuid>,
    /// The object class byte (`pcode`: 9 a prim, 47 an avatar, 255 a tree,
    /// 95 grass).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcode: Option<u8>,
    /// A comparison against the floating text an object shows over itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_text: Option<NameMatcher>,
    /// Only things near a point, ordered nearest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near: Option<Near>,
    /// Of the things that match everything else, only the one at this
    /// zero-based index — outwards from the anchor under [`near`](Self::near),
    /// in the viewer's stable order otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nth: Option<u32>,
}

impl WorldLocator {
    /// Things of `kind`.
    #[must_use]
    pub fn kind(kind: WorldKind) -> Self {
        Self {
            kind: Some(kind),
            ..Self::default()
        }
    }

    /// The own avatar.
    #[must_use]
    pub fn own_avatar() -> Self {
        Self {
            kind: Some(WorldKind::Avatar),
            own: Some(true),
            ..Self::default()
        }
    }

    /// The thing whose grid-wide id is `full_id`.
    #[must_use]
    pub fn full_id(full_id: Uuid) -> Self {
        Self {
            full_id: Some(full_id),
            ..Self::default()
        }
    }

    /// Also require it to be the own avatar or something it wears (`true`),
    /// or neither (`false`).
    #[must_use]
    pub const fn own(mut self, own: bool) -> Self {
        self.own = Some(own);
        self
    }

    /// Also require the name to be exactly `name`.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(NameMatcher::Exact(name.into()));
        self
    }

    /// Also require the name to contain `part`.
    #[must_use]
    pub fn name_containing(mut self, part: impl Into<String>) -> Self {
        self.name = Some(NameMatcher::Contains(part.into()));
        self
    }

    /// Also require the region-local id to be `local_id`.
    #[must_use]
    pub const fn local_id(mut self, local_id: u32) -> Self {
        self.local_id = Some(local_id);
        self
    }

    /// Also require the owner to be `owner`.
    #[must_use]
    pub const fn owned_by(mut self, owner: Uuid) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Also require the object class byte to be `pcode`.
    #[must_use]
    pub const fn pcode(mut self, pcode: u8) -> Self {
        self.pcode = Some(pcode);
        self
    }

    /// Also require the floating text to contain `part`.
    #[must_use]
    pub fn hover_text_containing(mut self, part: impl Into<String>) -> Self {
        self.hover_text = Some(NameMatcher::Contains(part.into()));
        self
    }

    /// Order the matches by their distance from `to`, nearest first, keeping
    /// only those within `radius` metres when it is set.
    #[must_use]
    pub const fn near(mut self, to: Anchor, radius: Option<f32>) -> Self {
        self.near = Some(Near { to, radius });
        self
    }

    /// The match nearest `to`: [`near`](Self::near) without a radius, and the
    /// first of its matches.
    #[must_use]
    pub const fn nearest_to(self, to: Anchor) -> Self {
        self.near(to, None).nth(0)
    }

    /// Pick the match at zero-based `index`.
    #[must_use]
    pub const fn nth(mut self, index: u32) -> Self {
        self.nth = Some(index);
        self
    }

    /// Whether `node` itself satisfies the criteria that concern a single
    /// thing: kind, own, name, ids, owner, class and floating text.
    ///
    /// [`near`](Self::near) and [`nth`](Self::nth) are not consulted — a
    /// distance needs the anchor's position and an index the other matches,
    /// which only the resolver knows. A criterion on a field the node does not
    /// have (yet) — a name that has not arrived — does not hold.
    #[must_use]
    pub fn matches_node(&self, node: &WorldNode) -> bool {
        self.kind.is_none_or(|kind| kind == node.kind)
            && self.own.is_none_or(|own| own == node.own)
            && matcher_holds(self.name.as_ref(), node.name.as_deref())
            && self.full_id.is_none_or(|id| id == node.full_id)
            && self.local_id.is_none_or(|id| node.local_id == Some(id))
            && self.owner.is_none_or(|owner| node.owner == Some(owner))
            && self.pcode.is_none_or(|pcode| pcode == node.pcode)
            && matcher_holds(self.hover_text.as_ref(), node.hover_text.as_deref())
    }
}

/// Whether an optional matcher holds over an optional text: unset holds
/// always, a set one needs the text.
fn matcher_holds(matcher: Option<&NameMatcher>, text: Option<&str>) -> bool {
    matcher.is_none_or(|matcher| text.is_some_and(|text| matcher.matches(text)))
}

/// Prints a selector-like description for error messages and logs —
/// `avatar own=true`, `object name="Door" near=own_avatar nth=0`. It is a
/// description, not a grammar: nothing parses it.
impl fmt::Display for WorldLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(kind) = self.kind {
            parts.push(kind.as_str().to_owned());
        }
        if let Some(own) = self.own {
            parts.push(format!("own={own}"));
        }
        match &self.name {
            Some(NameMatcher::Exact(name)) => parts.push(format!("name={name:?}")),
            Some(NameMatcher::Contains(part)) => parts.push(format!("name~={part:?}")),
            None => {}
        }
        if let Some(id) = self.full_id {
            parts.push(format!("#{id}"));
        }
        if let Some(id) = self.local_id {
            parts.push(format!("local={id}"));
        }
        if let Some(owner) = self.owner {
            parts.push(format!("owner={owner}"));
        }
        if let Some(pcode) = self.pcode {
            parts.push(format!("pcode={pcode}"));
        }
        match &self.hover_text {
            Some(NameMatcher::Exact(text)) => parts.push(format!("text={text:?}")),
            Some(NameMatcher::Contains(part)) => parts.push(format!("text~={part:?}")),
            None => {}
        }
        if let Some(near) = self.near {
            let to = match near.to {
                Anchor::Point([x, y, z]) => format!("<{x},{y},{z}>"),
                Anchor::OwnAvatar => "own_avatar".to_owned(),
            };
            match near.radius {
                Some(radius) => parts.push(format!("near={to} radius={radius}")),
                None => parts.push(format!("near={to}")),
            }
        }
        if let Some(index) = self.nth {
            parts.push(format!("nth={index}"));
        }
        if parts.is_empty() {
            f.write_str("*")
        } else {
            f.write_str(&parts.join(" "))
        }
    }
}

/// One in-world thing as a viewer tracks it.
///
/// Positions are **region-local** metres of the agent's current region, on
/// Second Life axes; a thing in a neighbouring region reads outside `0..256`.
/// Optional parts are left out of the JSON when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldNode {
    /// What it is.
    pub kind: WorldKind,
    /// The own avatar, or something it wears.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub own: bool,
    /// The grid-wide id: an object's full id, an avatar's agent id.
    pub full_id: Uuid,
    /// The region-local id it is streamed under; absent for an avatar known
    /// only from the coarse (minimap) locations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<u32>,
    /// The object class byte (`pcode`).
    pub pcode: u8,
    /// An object's name, or an avatar's shown name; absent until it arrives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// An object's description; absent until it arrives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// An object's owner (an agent, or a group); absent until it arrives, and
    /// for an avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Uuid>,
    /// Where it is drawn, region-local metres; absent for a HUD attachment,
    /// which is drawn on the screen rather than in the world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    /// Its rotation in the region, `[x, y, z, w]` with `w` not negative;
    /// absent with [`position`](Self::position).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 4]>,
    /// An object's size along each of its axes, metres; absent for an avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    /// The region-local id of what it hangs off: a child prim's linkset root,
    /// an attachment root's wearer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    /// The region-local ids of what hangs off it: a linkset root's child
    /// prims, an avatar's worn attachment roots. In ascending order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
    /// The attachment point an attachment is worn on (the attachment's own,
    /// for a child prim of it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_point: Option<u8>,
    /// For an attachment, the agent id of the avatar wearing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worn_by: Option<Uuid>,
    /// For an avatar, the region-local id of the object it sits on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sitting_on: Option<u32>,
    /// Whether the object is in the viewer's edit selection.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub selected: bool,
    /// The floating text an object shows over itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_text: Option<String>,
    /// What an avatar's name tag says, one line per line of the tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_tag: Option<String>,
}

/// One line naming the thing for a person reading an error — kind, name, ids
/// and position: `object "Door" #00000000-…-000000000001 local=1 at
/// <10,20,30>`.
impl fmt::Display for WorldNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.as_str())?;
        if self.own {
            f.write_str(" (own)")?;
        }
        if let Some(name) = &self.name {
            write!(f, " {name:?}")?;
        }
        write!(f, " #{}", self.full_id)?;
        if let Some(id) = self.local_id {
            write!(f, " local={id}")?;
        }
        match self.position {
            Some([x, y, z]) => write!(f, " at <{x},{y},{z}>"),
            None => f.write_str(" on the HUD"),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{Anchor, WorldKind, WorldLocator, WorldNode};

    /// A named prim with every optional part filled.
    fn door() -> WorldNode {
        WorldNode {
            kind: WorldKind::Object,
            own: false,
            full_id: Uuid::from_u128(1),
            local_id: Some(1),
            pcode: 9,
            name: Some("Door".to_owned()),
            description: Some("Opens".to_owned()),
            owner: Some(Uuid::from_u128(7)),
            position: Some([10.0, 20.0, 30.0]),
            rotation: Some([0.0, 0.0, 0.0, 1.0]),
            scale: Some([2.0, 3.0, 4.0]),
            parent: None,
            children: vec![2, 3],
            attachment_point: None,
            worn_by: None,
            sitting_on: None,
            selected: true,
            hover_text: Some("Open me".to_owned()),
            name_tag: None,
        }
    }

    #[test]
    fn each_criterion_can_reject() {
        let node = door();
        let cases = [
            (WorldLocator::default(), true),
            (WorldLocator::kind(WorldKind::Object), true),
            (WorldLocator::kind(WorldKind::Avatar), false),
            (WorldLocator::default().own(false), true),
            (WorldLocator::default().own(true), false),
            (WorldLocator::default().named("Door"), true),
            (WorldLocator::default().named("door"), false),
            (WorldLocator::default().name_containing("oo"), true),
            (WorldLocator::full_id(Uuid::from_u128(1)), true),
            (WorldLocator::full_id(Uuid::from_u128(2)), false),
            (WorldLocator::default().local_id(1), true),
            (WorldLocator::default().local_id(2), false),
            (WorldLocator::default().owned_by(Uuid::from_u128(7)), true),
            (WorldLocator::default().owned_by(Uuid::from_u128(8)), false),
            (WorldLocator::default().pcode(9), true),
            (WorldLocator::default().pcode(47), false),
            (WorldLocator::default().hover_text_containing("me"), true),
            (WorldLocator::default().hover_text_containing("you"), false),
        ];
        for (locator, expected) in cases {
            assert_eq!(locator.matches_node(&node), expected, "{locator}");
        }
    }

    #[test]
    fn a_criterion_on_a_missing_field_does_not_hold() {
        let mut node = door();
        node.name = None;
        node.owner = None;
        node.local_id = None;
        for locator in [
            WorldLocator::default().name_containing(""),
            WorldLocator::default().owned_by(Uuid::from_u128(7)),
            WorldLocator::default().local_id(1),
        ] {
            assert!(!locator.matches_node(&node), "{locator}");
        }
    }

    #[test]
    fn locators_and_nodes_round_trip() -> Result<(), serde_json::Error> {
        let locator = WorldLocator::kind(WorldKind::Attachment)
            .own(true)
            .named("Hat")
            .local_id(4)
            .owned_by(Uuid::from_u128(5))
            .pcode(9)
            .hover_text_containing("hi")
            .near(Anchor::Point([1.0, 2.0, 3.0]), Some(10.0))
            .nth(1);
        let json = serde_json::to_string(&locator)?;
        assert_eq!(
            serde_json::from_str::<WorldLocator>(&json)?,
            locator,
            "{json}"
        );
        let nearest = WorldLocator::kind(WorldKind::Object).nearest_to(Anchor::OwnAvatar);
        assert_eq!(
            serde_json::to_string(&nearest)?,
            r#"{"kind":"object","near":{"to":"own_avatar"},"nth":0}"#
        );
        let node = door();
        let json = serde_json::to_string(&node)?;
        assert_eq!(serde_json::from_str::<WorldNode>(&json)?, node, "{json}");
        Ok(())
    }

    #[test]
    fn misspelt_fields_are_refused() {
        for json in [
            r#"{"knd":"object"}"#,
            r#"{"near":{"to":"own_avatar","within_radius":3}}"#,
        ] {
            assert!(
                serde_json::from_str::<WorldLocator>(json).is_err(),
                "accepted {json}"
            );
        }
    }

    #[test]
    fn display_reads_like_a_selector() {
        let locator = WorldLocator::kind(WorldKind::Object)
            .named("Door")
            .near(Anchor::OwnAvatar, Some(5.0))
            .nth(0);
        assert_eq!(
            locator.to_string(),
            r#"object name="Door" near=own_avatar radius=5 nth=0"#
        );
        assert_eq!(WorldLocator::default().to_string(), "*");
        assert_eq!(
            door().to_string(),
            r#"object "Door" #00000000-0000-0000-0000-000000000001 local=1 at <10,20,30>"#
        );
    }
}
