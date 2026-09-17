//! What of the **own** avatar is drawn while the camera looks out of its eyes
//! (`viewer-mouselook-own-head-visible-from-inside`).
//!
//! The mouselook camera sits inside the head, so anything drawn there is seen
//! from the inside: the skull, the hair, a hat, a mesh head. The reference never
//! draws them in that view, and what it does instead depends on the
//! `FirstPersonAvatarVisible` setting ([`FirstPersonAvatarVisible`]):
//!
//! - **Body shown** — [`OwnAvatarView::Headless`]. The base head, hair and
//!   eyelashes are skipped when the view is drawn but still drawn into the
//!   shadow map (`LLVOAvatar::renderSkinned` gates them on
//!   `LLAgent::needsRenderHead() || LLPipeline::sShadowRender`), so the avatar's
//!   shadow keeps its head. The eyeballs are skipped in every pass
//!   (`renderRigid` has no shadow exception). An object worn on a point whose
//!   `visible_in_first_person` is off — the head points — is removed from every
//!   pass, shadows included (`LLVOAvatarSelf::updateAttachmentVisibility` zeroes
//!   its drawable type).
//! - **Body hidden** — [`OwnAvatarView::Hidden`]. Nothing of the avatar is drawn
//!   (`LLAgent::needsRenderAvatar`); `derender::hide_suppressed_avatars`, the one
//!   writer of an avatar anchor's visibility, hides the whole subtree.
//!
//! How each of those is expressed here follows from who else is writing what:
//!
//! - A base part's `Visibility` belongs to `apply_avatar_part_visibility` (bakes,
//!   alpha layers, the skirt), and a rigged submesh's to the bake-on-mesh and PBR
//!   face passes, so neither is touched. They are moved onto other **render
//!   layers** instead, with a leaf [`Propagate`] overriding the body root's: a
//!   head part onto [`dynamic_shadow_only_render_layers`] (the sun sees it, the
//!   main camera does not), an eyeball or a rigged submesh worn on a head point
//!   onto [`dynamic_probe_only_render_layers`] (neither does).
//! - A **rigid** attachment hangs off its point's node, and nothing else writes
//!   that node's `Visibility`, so the node is hidden. A layer would not reach
//!   through: the attachment's own object entity carries a `Propagate` of its
//!   own.
//!
//! Every override is recorded with a [`FirstPersonLayers`] marker and taken
//! back off when the view no longer wants it, so a part is only ever re-layered
//! by the system that layered it.
//!
//! Reference (Firestorm, read-only): `llagent.cpp` (`needsRenderAvatar`,
//! `needsRenderHead`), `llvoavatar.cpp` (`renderSkinned`, `renderTransparent`,
//! `renderRigid`), `llvoavatarself.cpp` (`updateAttachmentVisibility`),
//! `avatar_lad.xml` (`visible_in_first_person`).

use std::collections::HashMap;

use bevy::app::Propagate;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use sl_client_bevy::SlIdentity;

use crate::avatars::{AttachmentPointNode, AvatarBodyPart};
use sl_viewer_kit::avatar_assets::BodyRegion;
use sl_viewer_kit::probe_layers::{
    dynamic_probe_only_render_layers, dynamic_shadow_only_render_layers,
};
use sl_viewer_world_api::{
    AvatarPickTarget, AvatarState, CameraMode, FirstPersonAvatarVisible, ObjectState,
};
use sl_viewer_world_objects::objects::WornPickTarget;

/// How much of the own avatar the current camera mode draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnAvatarView {
    /// Everything: any mode but mouselook.
    Whole,
    /// Mouselook with the body shown: everything but the head and what is worn
    /// on it.
    Headless,
    /// Mouselook with the body hidden: nothing.
    Hidden,
}

impl OwnAvatarView {
    /// The view for camera `mode` under the `FirstPersonAvatarVisible` setting.
    #[must_use]
    pub const fn of(mode: CameraMode, body_visible: FirstPersonAvatarVisible) -> Self {
        match (mode, body_visible.0) {
            (CameraMode::Mouselook, true) => Self::Headless,
            (CameraMode::Mouselook, false) => Self::Hidden,
            (CameraMode::ThirdPerson | CameraMode::Flycam, _) => Self::Whole,
        }
    }

    /// The view the world is in right now, from resources a host may leave out:
    /// no camera mode is third person, and no setting is the default.
    #[must_use]
    pub fn current(
        mode: Option<&CameraMode>,
        body_visible: Option<&FirstPersonAvatarVisible>,
    ) -> Self {
        Self::of(
            mode.copied().unwrap_or_default(),
            body_visible.copied().unwrap_or_default(),
        )
    }

    /// The layers a base part of `region` is moved onto in this view, or `None`
    /// to leave it on the body root's.
    #[must_use]
    pub const fn base_part_layers(self, region: BodyRegion) -> Option<FirstPersonLayers> {
        match (self, region) {
            (Self::Headless, BodyRegion::Head | BodyRegion::Hair) => {
                Some(FirstPersonLayers::ShadowOnly)
            }
            (Self::Headless, BodyRegion::Eyes) => Some(FirstPersonLayers::ProbeOnly),
            (Self::Headless, BodyRegion::Upper | BodyRegion::Lower | BodyRegion::Skirt)
            | (Self::Whole | Self::Hidden, _) => None,
        }
    }

    /// Whether this view removes an object worn on a point whose
    /// `visible_in_first_person` is `visible_in_first_person`.
    #[must_use]
    pub const fn hides_worn_on(self, visible_in_first_person: bool) -> bool {
        matches!(self, Self::Headless) && !visible_in_first_person
    }
}

/// The render layers this module put on an own-avatar part or submesh, beside
/// the [`Propagate`] that carries them — the marker that says the override is
/// this module's to take back.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPersonLayers {
    /// Casts the sun's shadow, not drawn in the main view.
    ShadowOnly,
    /// Neither drawn in the main view nor shadowed.
    ProbeOnly,
}

impl FirstPersonLayers {
    /// The render layers this override stands for.
    #[must_use]
    pub fn render_layers(self) -> RenderLayers {
        match self {
            Self::ShadowOnly => dynamic_shadow_only_render_layers(),
            Self::ProbeOnly => dynamic_probe_only_render_layers(),
        }
    }
}

/// Apply [`OwnAvatarView::Headless`] to the own avatar, and take it back off in
/// any other view — see the [module documentation](self) for which lever each
/// piece is moved with and why.
///
/// A poll rather than a change-driven pass: a part is rebuilt, an attachment
/// worn or a mesh head re-rezzed while the camera stays in mouselook, and each
/// of those arrives as a fresh entity with no override on it.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the two inputs deciding the view, the identity and the \
              avatar and object mirrors it is resolved against, the three kinds of \
              entity it moves, and the commands that move them"
)]
pub(crate) fn apply_first_person_view(
    mode: Option<Res<CameraMode>>,
    body_visible: Option<Res<FirstPersonAvatarVisible>>,
    identity: Option<Res<SlIdentity>>,
    avatars: Res<AvatarState>,
    objects: Res<ObjectState>,
    parts: Query<(Entity, &AvatarBodyPart, Option<&FirstPersonLayers>)>,
    submeshes: Query<(
        Entity,
        &AvatarPickTarget,
        &WornPickTarget,
        Option<&FirstPersonLayers>,
    )>,
    mut nodes: Query<(&AttachmentPointNode, &mut Visibility)>,
    mut commands: Commands,
) {
    let view = OwnAvatarView::current(mode.as_deref(), body_visible.as_deref());
    let own = identity.as_deref().and_then(|identity| identity.agent_id);

    for (entity, part, current) in &parts {
        let wanted = if Some(part.agent()) == own {
            view.base_part_layers(part.region())
        } else {
            None
        };
        reconcile_layers(&mut commands, entity, current, wanted);
    }

    // The own body's points, each with its first-person flag, so a rigged
    // submesh can be judged by the point its object is worn on.
    let mut point_visible: HashMap<u8, bool> = HashMap::new();
    if let Some(own) = own {
        for (point, node) in avatars.attachment_nodes_of(own) {
            let Ok((marker, mut visibility)) = nodes.get_mut(node) else {
                continue;
            };
            point_visible.insert(point, marker.visible_in_first_person);
            let wanted = if view.hides_worn_on(marker.visible_in_first_person) {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            visibility.set_if_neq(wanted);
        }
    }

    for (entity, wearer, worn, current) in &submeshes {
        let hidden = Some(wearer.agent) == own
            && objects
                .attachment_point_of(worn.scoped)
                .and_then(|point| point_visible.get(&point).copied())
                .is_some_and(|visible| view.hides_worn_on(visible));
        let wanted = hidden.then_some(FirstPersonLayers::ProbeOnly);
        reconcile_layers(&mut commands, entity, current, wanted);
    }
}

/// Move `entity` onto the `wanted` override, or off the one it `current`ly has.
/// Taking an override off removes the [`Propagate`] with it, which puts the
/// entity back on the layers its parent propagates.
fn reconcile_layers(
    commands: &mut Commands,
    entity: Entity,
    current: Option<&FirstPersonLayers>,
    wanted: Option<FirstPersonLayers>,
) {
    match (current.copied(), wanted) {
        (Some(current), Some(wanted)) if current == wanted => {}
        (_, Some(wanted)) => {
            commands
                .entity(entity)
                .insert((Propagate(wanted.render_layers()), wanted));
        }
        (Some(_current), None) => {
            commands
                .entity(entity)
                .remove::<(Propagate<RenderLayers>, FirstPersonLayers)>();
        }
        (None, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{FirstPersonLayers, OwnAvatarView};
    use sl_viewer_kit::avatar_assets::BodyRegion;
    use sl_viewer_world_api::{CameraMode, FirstPersonAvatarVisible};

    /// Only mouselook changes what is drawn, and the setting decides between
    /// losing the head and losing everything.
    #[test]
    fn the_view_follows_the_mode_and_the_setting() {
        for body_visible in [true, false] {
            let setting = FirstPersonAvatarVisible(body_visible);
            assert_eq!(
                OwnAvatarView::of(CameraMode::ThirdPerson, setting),
                OwnAvatarView::Whole
            );
            assert_eq!(
                OwnAvatarView::of(CameraMode::Flycam, setting),
                OwnAvatarView::Whole
            );
        }
        assert_eq!(
            OwnAvatarView::of(CameraMode::Mouselook, FirstPersonAvatarVisible(true)),
            OwnAvatarView::Headless
        );
        assert_eq!(
            OwnAvatarView::of(CameraMode::Mouselook, FirstPersonAvatarVisible(false)),
            OwnAvatarView::Hidden
        );
        // A host with neither resource draws the whole avatar.
        assert_eq!(OwnAvatarView::current(None, None), OwnAvatarView::Whole);
    }

    /// With the body shown in mouselook, the head and hair keep their shadow,
    /// the eyeballs lose theirs too, and the body below the neck is untouched.
    #[test]
    fn a_headless_view_layers_exactly_the_head_parts() {
        let view = OwnAvatarView::Headless;
        let layered: Vec<(BodyRegion, Option<FirstPersonLayers>)> = [
            BodyRegion::Head,
            BodyRegion::Hair,
            BodyRegion::Eyes,
            BodyRegion::Upper,
            BodyRegion::Lower,
            BodyRegion::Skirt,
        ]
        .into_iter()
        .map(|region| (region, view.base_part_layers(region)))
        .collect();
        assert_eq!(
            layered,
            vec![
                (BodyRegion::Head, Some(FirstPersonLayers::ShadowOnly)),
                (BodyRegion::Hair, Some(FirstPersonLayers::ShadowOnly)),
                (BodyRegion::Eyes, Some(FirstPersonLayers::ProbeOnly)),
                (BodyRegion::Upper, None),
                (BodyRegion::Lower, None),
                (BodyRegion::Skirt, None),
            ]
        );
        for other in [OwnAvatarView::Whole, OwnAvatarView::Hidden] {
            assert_eq!(other.base_part_layers(BodyRegion::Head), None, "{other:?}");
        }
    }

    /// Only a headless view removes worn objects, and only those on a point
    /// that is not visible in first person. A hidden view removes everything
    /// through the anchor instead, so it needs nothing per point.
    #[test]
    fn only_a_headless_view_removes_what_is_worn_on_the_head() {
        assert!(OwnAvatarView::Headless.hides_worn_on(false));
        assert!(!OwnAvatarView::Headless.hides_worn_on(true));
        assert!(!OwnAvatarView::Whole.hides_worn_on(false));
        assert!(!OwnAvatarView::Hidden.hides_worn_on(false));
    }
}
