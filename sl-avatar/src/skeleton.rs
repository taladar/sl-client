//! Parsing of the standard Linden `character/` skeleton definition
//! (`avatar_skeleton.xml`) and the attachment-point / HUD-point table from
//! `avatar_lad.xml` (P12.2).
//!
//! Both files are client-side viewer assets, not fetched from the grid; this
//! module parses them from a borrowed `&str` (I/O-free, as the rest of the crate)
//! into index-linked models the Bevy layer later instantiates.
//!
//! - [`Skeleton::from_xml`] turns the nested `<bone>` hierarchy of
//!   `avatar_skeleton.xml` into a flat [`Joint`] list (parents before children)
//!   with rest transforms, pivots, and per-joint [`CollisionVolume`]s, plus
//!   name/alias lookup.
//! - [`AttachmentPoints::from_xml`] reads the `<attachment_point>` table nested
//!   under `avatar_lad.xml`'s `<skeleton>` element into the attachment-point →
//!   joint map and the HUD-point set, keyed by [`sl_proto::AttachmentPoint`].
//!
//! All transforms stay in Second Life's right-handed **Z-up** metre space and
//! rotations stay as the file's Euler XYZ **degrees**; the Bevy conversion (axis
//! swap, degree → quaternion) happens in `sl-client-bevy` at P13.

use std::collections::HashMap;

use sl_proto::AttachmentPoint;

/// An error returned while parsing a `character/` skeleton or attachment table.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `SkeletonError` reads clearly"
)]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SkeletonError {
    /// The XML itself was malformed and could not be parsed.
    #[error("malformed skeleton XML")]
    Xml(#[from] roxmltree::Error),
    /// The document's root element was not the one expected for this file.
    #[error("expected root element `{expected}`, found `{found}`")]
    UnexpectedRoot {
        /// The element name the parser required.
        expected: &'static str,
        /// The element name actually found.
        found: String,
    },
    /// A required child element was absent.
    #[error("missing required `{0}` element")]
    MissingElement(&'static str),
    /// A required attribute was absent on an element.
    #[error("`{element}` element is missing required attribute `{attribute}`")]
    MissingAttribute {
        /// The element that lacked the attribute.
        element: &'static str,
        /// The name of the missing attribute.
        attribute: &'static str,
    },
    /// An attribute expected to hold three space-separated floats could not be
    /// parsed as such.
    #[error("attribute `{attribute}` is not a 3-vector: {value:?}")]
    BadVector {
        /// The offending attribute name.
        attribute: &'static str,
        /// The raw attribute value.
        value: String,
    },
    /// A numeric attribute could not be parsed.
    #[error("attribute `{attribute}` is not a valid integer: {value:?}")]
    BadNumber {
        /// The offending attribute name.
        attribute: &'static str,
        /// The raw attribute value.
        value: String,
    },
}

/// A collision volume: a capsule hung off a joint that the viewer uses for
/// avatar shape and camera/click tests. Purely descriptive here — the shape
/// math consuming the transform lives in later phases.
#[derive(Clone, Debug, PartialEq)]
pub struct CollisionVolume {
    /// The volume's name (e.g. `PELVIS`, `CHEST`), unique within the skeleton.
    pub name: String,
    /// Local translation from the owning joint, in metres (Z-up).
    pub pos: [f32; 3],
    /// Local rotation as Euler XYZ angles, in degrees (as written in the XML).
    pub rot: [f32; 3],
    /// Local scale (unitless multipliers).
    pub scale: [f32; 3],
    /// The `end` vector (the volume's principal-axis tip offset), in metres.
    pub end: [f32; 3],
}

/// Which skeleton a joint belongs to: the original **base** avatar skeleton or the
/// later **extended** (Bento) additions — the `support` attribute on an
/// `avatar_skeleton.xml` `<bone>` (Firestorm's `LLJoint::SUPPORT_BASE` /
/// `SUPPORT_EXTENDED`).
///
/// This distinction drives the base-mesh joint-render-data ordering: a legacy
/// system-avatar part's per-vertex weights index a list built over the *base*
/// skeleton only, so when rebuilding that list the reference viewer walks past any
/// extended joint that sits between a base joint and its base ancestor
/// (`getBaseSkeletonAncestor`, SL-287). An absent `support` attribute defaults to
/// [`Base`](Self::Base), matching the reference loader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JointSupport {
    /// A joint of the original base avatar skeleton (`support="base"`, or absent).
    #[default]
    Base,
    /// An extended (Bento) joint added on top of the base skeleton
    /// (`support="extended"`).
    Extended,
}

/// A single skeleton joint (bone) with its rest transform and hierarchy links.
///
/// Positions/rotations are the bone's rest pose *relative to its parent*; index
/// [`parent`](Self::parent) / [`children`](Self::children) into
/// [`Skeleton::joints`].
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    /// The joint's canonical name (e.g. `mPelvis`).
    pub name: String,
    /// Alternate names the asset pipeline accepts for this joint (the
    /// space-separated `aliases` attribute), e.g. `hip`, `avatar_mPelvis`.
    pub aliases: Vec<String>,
    /// The parent joint's index within [`Skeleton::joints`], or `None` for a
    /// root joint.
    pub parent: Option<usize>,
    /// Indices of this joint's child joints within [`Skeleton::joints`].
    pub children: Vec<usize>,
    /// Whether this joint is rigidly connected to its parent (the `connected`
    /// attribute); a disconnected joint floats free of the parent's tip.
    pub connected: bool,
    /// The joint group the viewer buckets this bone under (`Torso`, `Spine`,
    /// `Face`, …), if present.
    pub group: Option<String>,
    /// Whether this bone is part of the base skeleton or an extended (Bento)
    /// addition (the `support` attribute; absent defaults to
    /// [`Base`](JointSupport::Base)).
    pub support: JointSupport,
    /// The bone's rest translation from its parent, in metres (Z-up).
    pub pos: [f32; 3],
    /// The bone's rest rotation as Euler XYZ angles, in degrees.
    pub rot: [f32; 3],
    /// The bone's rest scale (unitless multipliers).
    pub scale: [f32; 3],
    /// The joint pivot offset used when posing a connected bone, in metres.
    pub pivot: [f32; 3],
    /// The bone's `end` vector (tip offset from its own origin), in metres.
    pub end: [f32; 3],
    /// Collision volumes hung off this joint (may be empty).
    pub collision_volumes: Vec<CollisionVolume>,
}

/// A parsed Linden avatar skeleton: a flat [`Joint`] list in document order
/// (each parent precedes its descendants) plus name/alias → index lookup.
#[derive(Clone, Debug)]
pub struct Skeleton {
    /// All joints, in document order. Reach into this with the accessors.
    joints: Vec<Joint>,
    /// Indices of the root joints (those with no parent), in document order.
    roots: Vec<usize>,
    /// Lookup from a joint's canonical name *or* any of its aliases to its
    /// index. A canonical name always wins over an alias on collision.
    lookup: HashMap<String, usize>,
    /// The bone count the document declared in its `num_bones` attribute, if
    /// present (metadata; the parser trusts the actual element count).
    declared_bone_count: Option<usize>,
    /// The collision-volume count the document declared in its
    /// `num_collision_volumes` attribute, if present.
    declared_collision_volume_count: Option<usize>,
}

impl Skeleton {
    /// Parse an `avatar_skeleton.xml` document from its text.
    ///
    /// # Errors
    ///
    /// Returns [`SkeletonError`] if the XML is malformed, the root is not
    /// `<linden_skeleton>`, a bone/collision-volume lacks a required attribute,
    /// or a transform attribute is not three floats.
    pub fn from_xml(xml: &str) -> Result<Self, SkeletonError> {
        let doc = roxmltree::Document::parse(xml)?;
        let root = doc.root_element();
        if root.tag_name().name() != "linden_skeleton" {
            return Err(SkeletonError::UnexpectedRoot {
                expected: "linden_skeleton",
                found: root.tag_name().name().to_owned(),
            });
        }

        let declared_bone_count = parse_opt_usize(root, "num_bones")?;
        let declared_collision_volume_count = parse_opt_usize(root, "num_collision_volumes")?;

        let mut joints = Vec::new();
        let mut roots = Vec::new();
        for bone in root
            .children()
            .filter(|node| node.is_element() && node.tag_name().name() == "bone")
        {
            let raw = parse_raw_bone(bone)?;
            roots.push(flatten(raw, None, &mut joints));
        }

        let mut lookup = HashMap::new();
        for (index, joint) in joints.iter().enumerate() {
            // Aliases first so a canonical name always overwrites an alias that
            // happens to collide with it.
            for alias in &joint.aliases {
                lookup.entry(alias.clone()).or_insert(index);
            }
        }
        for (index, joint) in joints.iter().enumerate() {
            lookup.insert(joint.name.clone(), index);
        }

        Ok(Self {
            joints,
            roots,
            lookup,
            declared_bone_count,
            declared_collision_volume_count,
        })
    }

    /// All joints, in document order (parents before their descendants).
    #[must_use]
    pub fn joints(&self) -> &[Joint] {
        &self.joints
    }

    /// The number of joints.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.joints.len()
    }

    /// Whether the skeleton has no joints.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.joints.is_empty()
    }

    /// The joint at `index`, or `None` if out of range.
    #[must_use]
    pub fn joint(&self, index: usize) -> Option<&Joint> {
        self.joints.get(index)
    }

    /// The indices of the root joints (those with no parent).
    #[must_use]
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    /// The index of the joint with the given canonical name or alias.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<usize> {
        self.lookup.get(name).copied()
    }

    /// The joint with the given canonical name or alias.
    #[must_use]
    pub fn joint_by_name(&self, name: &str) -> Option<&Joint> {
        self.find(name).and_then(|index| self.joints.get(index))
    }

    /// The parent of the joint at `index`, if any.
    #[must_use]
    pub fn parent_of(&self, index: usize) -> Option<&Joint> {
        self.joints
            .get(index)
            .and_then(|joint| joint.parent)
            .and_then(|parent| self.joints.get(parent))
    }

    /// The child joints of the joint at `index`, in document order.
    #[must_use]
    pub fn children_of(&self, index: usize) -> Vec<&Joint> {
        self.joints
            .get(index)
            .map(|joint| {
                joint
                    .children
                    .iter()
                    .filter_map(|&child| self.joints.get(child))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The total number of collision volumes across all joints.
    #[must_use]
    pub fn collision_volume_count(&self) -> usize {
        self.joints.iter().fold(0_usize, |total, joint| {
            total.saturating_add(joint.collision_volumes.len())
        })
    }

    /// The bone count the document declared in `num_bones`, if present.
    #[must_use]
    pub const fn declared_bone_count(&self) -> Option<usize> {
        self.declared_bone_count
    }

    /// The collision-volume count declared in `num_collision_volumes`, if
    /// present.
    #[must_use]
    pub const fn declared_collision_volume_count(&self) -> Option<usize> {
        self.declared_collision_volume_count
    }
}

/// An intermediate, owned bone subtree built while flattening the nested XML
/// into [`Skeleton::joints`]'s index-linked form.
struct RawBone {
    /// The joint's canonical name.
    name: String,
    /// The joint's aliases.
    aliases: Vec<String>,
    /// Whether the bone is connected to its parent.
    connected: bool,
    /// The joint's group, if present.
    group: Option<String>,
    /// Whether the bone is a base-skeleton or extended (Bento) joint.
    support: JointSupport,
    /// Rest translation from the parent.
    pos: [f32; 3],
    /// Rest rotation (Euler XYZ degrees).
    rot: [f32; 3],
    /// Rest scale.
    scale: [f32; 3],
    /// Pivot offset.
    pivot: [f32; 3],
    /// The bone's `end` vector.
    end: [f32; 3],
    /// Collision volumes hung off this bone.
    collision_volumes: Vec<CollisionVolume>,
    /// The nested child bones.
    children: Vec<Self>,
}

/// Parse one `<bone>` element (and its nested bones/collision volumes)
/// recursively into a [`RawBone`].
fn parse_raw_bone(node: roxmltree::Node<'_, '_>) -> Result<RawBone, SkeletonError> {
    let name = req_attr(node, "bone", "name")?.to_owned();
    let aliases = node
        .attribute("aliases")
        .map(|value| value.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    let connected = node.attribute("connected") == Some("true");
    let group = node.attribute("group").map(str::to_owned);
    // An absent `support` attribute defaults to base (Firestorm's loader).
    let support = match node.attribute("support") {
        Some("extended") => JointSupport::Extended,
        _ => JointSupport::Base,
    };
    let pos = vec3_attr(node, "bone", "pos")?;
    let rot = vec3_attr(node, "bone", "rot")?;
    let scale = vec3_attr(node, "bone", "scale")?;
    let pivot = vec3_attr(node, "bone", "pivot")?;
    let end = vec3_attr(node, "bone", "end")?;

    let mut collision_volumes = Vec::new();
    let mut children = Vec::new();
    for child in node.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "bone" => children.push(parse_raw_bone(child)?),
            "collision_volume" => collision_volumes.push(parse_collision_volume(child)?),
            _ => {}
        }
    }

    Ok(RawBone {
        name,
        aliases,
        connected,
        group,
        support,
        pos,
        rot,
        scale,
        pivot,
        end,
        collision_volumes,
        children,
    })
}

/// Flatten a [`RawBone`] subtree into `joints`, linking parent/child indices,
/// and return the index the subtree's root joint was placed at.
fn flatten(raw: RawBone, parent: Option<usize>, joints: &mut Vec<Joint>) -> usize {
    let index = joints.len();
    joints.push(Joint {
        name: raw.name,
        aliases: raw.aliases,
        parent,
        children: Vec::new(),
        connected: raw.connected,
        group: raw.group,
        support: raw.support,
        pos: raw.pos,
        rot: raw.rot,
        scale: raw.scale,
        pivot: raw.pivot,
        end: raw.end,
        collision_volumes: raw.collision_volumes,
    });

    let mut child_indices = Vec::new();
    for child in raw.children {
        child_indices.push(flatten(child, Some(index), joints));
    }
    if let Some(joint) = joints.get_mut(index) {
        joint.children = child_indices;
    }
    index
}

/// Parse one `<collision_volume>` element.
fn parse_collision_volume(node: roxmltree::Node<'_, '_>) -> Result<CollisionVolume, SkeletonError> {
    Ok(CollisionVolume {
        name: req_attr(node, "collision_volume", "name")?.to_owned(),
        pos: vec3_attr(node, "collision_volume", "pos")?,
        rot: vec3_attr(node, "collision_volume", "rot")?,
        scale: vec3_attr(node, "collision_volume", "scale")?,
        end: vec3_attr(node, "collision_volume", "end")?,
    })
}

/// One attachment point from `avatar_lad.xml`: which joint an attached object
/// hangs from, its default local offset, and whether it is a HUD slot.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentPointDef {
    /// The typed attachment point, mapped from the numeric `id`.
    pub point: AttachmentPoint,
    /// The raw numeric id (`1`..=`55`) as written in the XML.
    pub id: u8,
    /// The human-readable name (e.g. `Chest`, `Left Hand`, `Center`).
    pub name: String,
    /// The skeleton joint this point hangs from (e.g. `mChest`; HUD points hang
    /// from the pseudo-joint `mScreen`).
    pub joint: String,
    /// The `ATTACH_*` location symbol, if present.
    pub location: Option<String>,
    /// The point's default local translation, in metres.
    pub position: [f32; 3],
    /// The point's default local rotation as Euler XYZ angles, in degrees.
    pub rotation: [f32; 3],
    /// Whether this is a screen-space HUD point (the `hud="true"` attribute)
    /// rather than a world-space body point.
    pub is_hud: bool,
}

/// The parsed attachment-point table from `avatar_lad.xml`: the
/// attachment-point → joint mapping plus the HUD-point set.
#[derive(Clone, Debug)]
pub struct AttachmentPoints {
    /// All attachment points, in document order.
    points: Vec<AttachmentPointDef>,
}

impl AttachmentPoints {
    /// Parse the `<attachment_point>` table nested under `avatar_lad.xml`'s
    /// `<skeleton>` element.
    ///
    /// # Errors
    ///
    /// Returns [`SkeletonError`] if the XML is malformed, the root is not
    /// `<linden_avatar>`, the `<skeleton>` element is missing, or an
    /// `<attachment_point>` lacks a required attribute / has a malformed vector
    /// or id.
    pub fn from_xml(xml: &str) -> Result<Self, SkeletonError> {
        let doc = roxmltree::Document::parse(xml)?;
        let root = doc.root_element();
        if root.tag_name().name() != "linden_avatar" {
            return Err(SkeletonError::UnexpectedRoot {
                expected: "linden_avatar",
                found: root.tag_name().name().to_owned(),
            });
        }
        let skeleton = root
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "skeleton")
            .ok_or(SkeletonError::MissingElement("skeleton"))?;

        let mut points = Vec::new();
        for node in skeleton
            .children()
            .filter(|node| node.is_element() && node.tag_name().name() == "attachment_point")
        {
            points.push(parse_attachment_point(node)?);
        }
        Ok(Self { points })
    }

    /// All attachment points, in document order.
    #[must_use]
    pub fn all(&self) -> &[AttachmentPointDef] {
        &self.points
    }

    /// The definition for a given attachment point, if the table defines it.
    #[must_use]
    pub fn get(&self, point: AttachmentPoint) -> Option<&AttachmentPointDef> {
        self.points.iter().find(|def| def.point == point)
    }

    /// The skeleton joint an attachment point hangs from, if defined.
    #[must_use]
    pub fn joint_for(&self, point: AttachmentPoint) -> Option<&str> {
        self.get(point).map(|def| def.joint.as_str())
    }

    /// Whether the given attachment point is a HUD (screen-space) point per the
    /// table.
    #[must_use]
    pub fn is_hud(&self, point: AttachmentPoint) -> bool {
        self.get(point).is_some_and(|def| def.is_hud)
    }

    /// The attachment-point → joint mapping, in document order.
    #[must_use]
    pub fn joint_map(&self) -> Vec<(AttachmentPoint, &str)> {
        self.points
            .iter()
            .map(|def| (def.point, def.joint.as_str()))
            .collect()
    }

    /// The set of HUD attachment points, in document order.
    #[must_use]
    pub fn hud_points(&self) -> Vec<AttachmentPoint> {
        self.points
            .iter()
            .filter(|def| def.is_hud)
            .map(|def| def.point)
            .collect()
    }
}

/// Parse one `<attachment_point>` element.
fn parse_attachment_point(
    node: roxmltree::Node<'_, '_>,
) -> Result<AttachmentPointDef, SkeletonError> {
    let id_str = req_attr(node, "attachment_point", "id")?;
    let id: u8 = match id_str.parse() {
        Ok(value) => value,
        Err(_) => {
            return Err(SkeletonError::BadNumber {
                attribute: "id",
                value: id_str.to_owned(),
            });
        }
    };
    Ok(AttachmentPointDef {
        point: AttachmentPoint::from_code(id),
        id,
        name: req_attr(node, "attachment_point", "name")?.to_owned(),
        joint: req_attr(node, "attachment_point", "joint")?.to_owned(),
        location: node.attribute("location").map(str::to_owned),
        position: vec3_attr(node, "attachment_point", "position")?,
        rotation: vec3_attr(node, "attachment_point", "rotation")?,
        is_hud: node.attribute("hud") == Some("true"),
    })
}

/// Return a required attribute or a [`SkeletonError::MissingAttribute`].
fn req_attr<'a>(
    node: roxmltree::Node<'a, '_>,
    element: &'static str,
    attribute: &'static str,
) -> Result<&'a str, SkeletonError> {
    node.attribute(attribute)
        .ok_or(SkeletonError::MissingAttribute { element, attribute })
}

/// Parse a required attribute as three space-separated floats.
fn vec3_attr(
    node: roxmltree::Node<'_, '_>,
    element: &'static str,
    attribute: &'static str,
) -> Result<[f32; 3], SkeletonError> {
    parse_vec3(req_attr(node, element, attribute)?, attribute)
}

/// Parse exactly three space-separated `f32`s from `value`.
fn parse_vec3(value: &str, attribute: &'static str) -> Result<[f32; 3], SkeletonError> {
    let mut parts = value.split_whitespace();
    let mut out = [0.0_f32; 3];
    for slot in &mut out {
        let token = parts.next().ok_or_else(|| SkeletonError::BadVector {
            attribute,
            value: value.to_owned(),
        })?;
        match token.parse::<f32>() {
            Ok(parsed) => *slot = parsed,
            Err(_) => {
                return Err(SkeletonError::BadVector {
                    attribute,
                    value: value.to_owned(),
                });
            }
        }
    }
    if parts.next().is_some() {
        return Err(SkeletonError::BadVector {
            attribute,
            value: value.to_owned(),
        });
    }
    Ok(out)
}

/// Parse an optional unsigned-integer attribute (absent → `Ok(None)`).
fn parse_opt_usize(
    node: roxmltree::Node<'_, '_>,
    attribute: &'static str,
) -> Result<Option<usize>, SkeletonError> {
    match node.attribute(attribute) {
        None => Ok(None),
        Some(value) => match value.parse::<usize>() {
            Ok(parsed) => Ok(Some(parsed)),
            Err(_) => Err(SkeletonError::BadNumber {
                attribute,
                value: value.to_owned(),
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{AttachmentPoints, Skeleton, SkeletonError};
    use pretty_assertions::assert_eq;
    use sl_proto::AttachmentPoint;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A minimal committed skeleton fixture: 4 bones, 2 collision volumes.
    const MINI_SKELETON: &str = include_str!("../tests/fixtures/mini_skeleton.xml");
    /// A minimal committed attachment fixture: chest, skull, one HUD point.
    const MINI_LAD: &str = include_str!("../tests/fixtures/mini_lad.xml");

    /// Compare two vectors within a tolerance (parsed floats are exact for
    /// these fixtures, but the epsilon keeps the assertion off `float_cmp`).
    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn parses_hierarchy_and_counts() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        assert_eq!(skeleton.len(), 4);
        assert_eq!(skeleton.declared_bone_count(), Some(4));
        assert_eq!(skeleton.declared_collision_volume_count(), Some(2));
        assert_eq!(skeleton.collision_volume_count(), 2);

        // Single root: mPelvis.
        assert_eq!(skeleton.roots().len(), 1);
        let pelvis_idx = skeleton.find("mPelvis").ok_or("pelvis present")?;
        assert_eq!(skeleton.roots().first().copied(), Some(pelvis_idx));
        let pelvis = skeleton.joint(pelvis_idx).ok_or("pelvis joint")?;
        assert_eq!(pelvis.parent, None);
        assert!(!pelvis.connected);

        // mPelvis has two children: mTorso and mHipRight.
        let child_names: Vec<&str> = skeleton
            .children_of(pelvis_idx)
            .iter()
            .map(|joint| joint.name.as_str())
            .collect();
        assert_eq!(child_names, ["mTorso", "mHipRight"]);
        Ok(())
    }

    #[test]
    fn resolves_aliases_and_parent_links() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;

        // Aliases resolve to the same joint as the canonical name.
        assert_eq!(skeleton.find("mPelvis"), skeleton.find("hip"));
        assert_eq!(skeleton.find("mPelvis"), skeleton.find("avatar_mPelvis"));
        assert_eq!(skeleton.find("mChest"), skeleton.find("chest"));

        // mChest's parent chain: mChest -> mTorso -> mPelvis.
        let chest_idx = skeleton.find("mChest").ok_or("chest present")?;
        let torso = skeleton.parent_of(chest_idx).ok_or("chest has a parent")?;
        assert_eq!(torso.name, "mTorso");
        let torso_idx = skeleton.find("mTorso").ok_or("torso present")?;
        let pelvis = skeleton.parent_of(torso_idx).ok_or("torso has a parent")?;
        assert_eq!(pelvis.name, "mPelvis");

        // Parents always precede their children in document order.
        for (index, joint) in skeleton.joints().iter().enumerate() {
            if let Some(parent) = joint.parent {
                assert!(parent < index, "parent {parent} must precede child {index}");
            }
        }
        Ok(())
    }

    #[test]
    fn parses_transforms_and_collision_volumes() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let pelvis = skeleton.joint_by_name("mPelvis").ok_or("pelvis present")?;
        assert!(close(pelvis.pos, [0.0, 0.0, 1.067]));
        assert!(close(pelvis.pivot, [0.0, 0.0, 1.067_015]));
        assert!(close(pelvis.scale, [1.0, 1.0, 1.0]));
        assert_eq!(pelvis.group.as_deref(), Some("Torso"));

        // The pelvis carries the PELVIS collision volume.
        assert_eq!(pelvis.collision_volumes.len(), 1);
        let volume = pelvis.collision_volumes.first().ok_or("pelvis volume")?;
        assert_eq!(volume.name, "PELVIS");
        assert!(close(volume.scale, [0.12, 0.16, 0.17]));
        Ok(())
    }

    #[test]
    fn rejects_wrong_root() {
        let result = Skeleton::from_xml("<linden_avatar/>");
        assert!(matches!(result, Err(SkeletonError::UnexpectedRoot { .. })));
    }

    #[test]
    fn parses_attachment_points_and_hud_set() -> Result<(), TestError> {
        let points = AttachmentPoints::from_xml(MINI_LAD)?;
        assert_eq!(points.all().len(), 3);

        // The attachment-point -> joint map.
        assert_eq!(points.joint_for(AttachmentPoint::Chest), Some("mChest"));
        assert_eq!(points.joint_for(AttachmentPoint::Skull), Some("mHead"));
        assert_eq!(
            points.joint_for(AttachmentPoint::HudCenter),
            Some("mScreen")
        );
        assert_eq!(points.joint_for(AttachmentPoint::LeftHand), None);

        // Ids map onto the typed enum.
        let chest = points.get(AttachmentPoint::Chest).ok_or("chest present")?;
        assert_eq!(chest.id, 1);
        assert!(!chest.is_hud);
        assert_eq!(chest.location.as_deref(), Some("ATTACH_CHEST"));
        assert!(close(chest.position, [0.15, 0.0, -0.1]));

        // Only the HUD point lands in the HUD set, and it agrees with the wire
        // enum's own HUD classification.
        assert_eq!(points.hud_points(), vec![AttachmentPoint::HudCenter]);
        assert!(points.is_hud(AttachmentPoint::HudCenter));
        assert!(AttachmentPoint::HudCenter.is_hud());
        assert!(!points.is_hud(AttachmentPoint::Chest));
        Ok(())
    }

    #[test]
    fn joint_map_covers_every_point_in_order() -> Result<(), TestError> {
        let points = AttachmentPoints::from_xml(MINI_LAD)?;
        let map = points.joint_map();
        assert_eq!(
            map,
            vec![
                (AttachmentPoint::Chest, "mChest"),
                (AttachmentPoint::Skull, "mHead"),
                (AttachmentPoint::HudCenter, "mScreen"),
            ]
        );
        Ok(())
    }
}
