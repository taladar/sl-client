//! Showing a decoded texture on a UI node.
//!
//! A profile picture, a group insignia, an inventory thumbnail, the texture
//! picker's preview, a landmark's snapshot and a search result's image are the
//! same operation: request a texture at the boost priority a surface someone is
//! looking at uses, park the node that will show it, and swap the decoded image
//! in when it lands. It was written eight times, once per floater, and every
//! copy ended in a bare `images.add(upload_decoded(..))` — a fresh full RGBA
//! upload with no dedup and nothing that ever drops it, so the same thumbnail
//! shown in the gallery, the item's properties window and a profile was three
//! copies of itself, and re-opening a window was another one each time, all of
//! them alive until the viewer exited.
//!
//! The world layer had already solved it five times over (prim faces, legacy
//! materials, terrain, avatar bakes all keep a `TextureKey -> Handle<Image>`
//! map); this is that map for the UI tier, with the polling written once:
//!
//! - [`UiTextureImages`] holds the uploads, keyed by texture id **and** the
//!   level of detail they were built from, so a texture that re-decodes finer
//!   is uploaded again rather than staying coarse forever;
//! - the map holds them **weakly** ([`AssetId`], not [`Handle`]), so the nodes
//!   showing an image are what keeps it alive. When the last window showing a
//!   thumbnail closes, Bevy drops the image and the next window that wants it
//!   uploads it again — the memory is bounded by what is on screen rather than
//!   by everything the session has ever displayed;
//! - [`PendingUiTexture`] on the node says what it is waiting for, and the
//!   plugin's poll paints every waiting node from the one map.
//!
//! The subject is the **node**, not a list held by the window: a rebuilt or
//! closed window takes its pending textures with it, so a decode that lands
//! afterwards has nothing to paint. Each of the per-window lists this replaced
//! had to remember to check that for itself, and a texture picked twice in quick
//! succession could still let the first decode land on the pane after the
//! second.

use bevy::prelude::*;
use sl_client_bevy::{DecodedTexture, DiscardLevel, TextureKey, TextureUpload, upload_decoded};

use crate::{AVATAR_BOOST_PRIORITY, BoostTexture, DecodedTextures};
use std::collections::HashMap;

/// A UI node waiting for a texture to decode.
///
/// Inserting it is the whole request: [`UiTexturePlugin`]'s systems ask for the
/// texture at [`AVATAR_BOOST_PRIORITY`] and swap the decoded image onto the node
/// as an [`ImageNode`], then remove the component. Insert it again — with the
/// same key or a different one — to point the same box at another texture; the
/// newest insertion is the one that lands.
#[expect(
    clippy::module_name_repetitions,
    reason = "named for the caller, who imports it into a crate where `ui_texture` is not \
              in scope and `PendingUiTexture` is what reads clearly"
)]
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingUiTexture {
    /// The texture the node is waiting for.
    key: TextureKey,
    /// Whether the node's children are a placeholder — a "(loading)" label —
    /// to drop once the image is in.
    over_placeholder: bool,
}

impl PendingUiTexture {
    /// Wait for `key` on a node that shows nothing meanwhile: an empty swatch,
    /// a preview pane, a box the image simply appears in.
    #[must_use]
    pub const fn new(key: TextureKey) -> Self {
        Self {
            key,
            over_placeholder: false,
        }
    }

    /// Wait for `key` on a box holding a placeholder — the "(loading)" label the
    /// profile / insignia / snapshot boxes spawn — whose children are despawned
    /// when the image lands.
    #[must_use]
    pub const fn over_placeholder(key: TextureKey) -> Self {
        Self {
            key,
            over_placeholder: true,
        }
    }

    /// The texture this node is waiting for.
    #[must_use]
    pub const fn key(&self) -> TextureKey {
        self.key
    }
}

/// One uploaded image, as [`UiTextureImages`] remembers it.
#[derive(Debug, Clone, Copy)]
struct UploadedImage {
    /// The level of detail the pixels came from. A texture that re-decodes at a
    /// finer level is a different image, so the upload is redone rather than the
    /// coarse one served forever.
    level: DiscardLevel,
    /// The uploaded image, held as an id rather than a [`Handle`] so the map
    /// does not keep it alive — see the module docs.
    id: AssetId<Image>,
}

/// The Bevy images the UI tier has uploaded, deduplicated by texture id.
///
/// Every surface that shows a texture goes through [`image`](Self::image), so
/// one thumbnail shown in five places is one image, and a window re-opened ten
/// times uploads nothing new.
#[expect(
    clippy::module_name_repetitions,
    reason = "named for the caller, who sees a resource beside `DecodedTextures` rather \
              than an item of this module"
)]
#[derive(Resource, Default, Debug)]
pub struct UiTextureImages {
    /// The uploads, by texture id.
    uploaded: HashMap<TextureKey, UploadedImage>,
}

impl UiTextureImages {
    /// The Bevy image for `decoded`, uploading it the first time and reusing it
    /// after.
    ///
    /// Re-uploads when the decoded texture is at a different level of detail
    /// than the recorded upload, or when the image has since been dropped
    /// (nothing was showing it, so Bevy released it).
    pub fn image(
        &mut self,
        key: TextureKey,
        decoded: &DecodedTexture,
        images: &mut Assets<Image>,
    ) -> Handle<Image> {
        if let Some(uploaded) = self.uploaded.get(&key).copied()
            && uploaded.level == decoded.discard_level
            && let Some(handle) = images.get_strong_handle(uploaded.id)
        {
            return handle;
        }
        let handle = images.add(upload_decoded(decoded, TextureUpload::COLOR));
        let _replaced = self.uploaded.insert(
            key,
            UploadedImage {
                level: decoded.discard_level,
                id: handle.id(),
            },
        );
        handle
    }

    /// How many textures have an upload recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.uploaded.len()
    }

    /// Whether nothing has been uploaded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.uploaded.is_empty()
    }
}

/// Ask the texture pipeline for every texture a node has just started waiting
/// for, at the priority a surface the user is looking at asks with.
///
/// Reacts to the component **changing**, not merely being added, so pointing an
/// existing box at a second texture (the picker's preview pane following the
/// selection) asks for that one too.
fn boost_pending_ui_textures(
    pending: Query<&PendingUiTexture, Changed<PendingUiTexture>>,
    mut boost: MessageWriter<BoostTexture>,
) {
    for want in &pending {
        boost.write(BoostTexture {
            key: want.key,
            priority: AVATAR_BOOST_PRIORITY,
        });
    }
}

/// Swap the decoded image onto every waiting node, dropping the placeholder the
/// box was holding.
fn poll_ui_textures(
    pending: Query<(Entity, &PendingUiTexture)>,
    store: Res<DecodedTextures>,
    mut uploaded: ResMut<UiTextureImages>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for (node, want) in &pending {
        let Some(decoded) = store.get(want.key) else {
            continue;
        };
        let handle = uploaded.image(want.key, decoded, &mut images);
        commands
            .entity(node)
            .insert(ImageNode::new(handle))
            .remove::<PendingUiTexture>();
        if want.over_placeholder
            && let Ok(kids) = children.get(node)
        {
            for child in kids.iter().collect::<Vec<Entity>>() {
                commands.entity(child).despawn();
            }
        }
    }
}

/// Wires the shared UI-texture map and its poll.
///
/// Added by each floater crate's plugin behind an `is_plugin_added` guard — the
/// eight surfaces that show textures live in six crates, none of which owns the
/// others, so the first to build it wins and a test app that adds only one of
/// them still paints its images.
#[expect(
    clippy::module_name_repetitions,
    reason = "a plugin is named where an app adds it, and `UiTexturePlugin` is what says \
              which plugin that is"
)]
#[derive(Debug)]
pub struct UiTexturePlugin;

impl Plugin for UiTexturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiTextureImages>()
            .init_resource::<DecodedTextures>()
            .add_message::<BoostTexture>()
            .add_systems(
                Update,
                (boost_pending_ui_textures, poll_ui_textures).chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingUiTexture, UiTextureImages, UiTexturePlugin};
    use crate::{AVATAR_BOOST_PRIORITY, BoostTexture, DecodedTextures};
    use bevy::prelude::*;
    use bytes::Bytes;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{DecodedTexture, DiscardLevel, TextureKey, Uuid};
    use std::sync::Arc;

    /// What a test that cannot go on reports.
    type TestError = Box<dyn core::error::Error>;

    /// An app with the shared poll and the `Assets<Image>` it uploads into.
    fn texture_app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .add_plugins(UiTexturePlugin);
        app
    }

    /// The nth test texture id.
    fn key(id: u128) -> TextureKey {
        TextureKey::from(Uuid::from_u128(id))
    }

    /// A one-pixel decoded texture at `level`.
    fn decoded(level: DiscardLevel) -> Arc<DecodedTexture> {
        Arc::new(DecodedTexture::new(
            1,
            1,
            4,
            level,
            Bytes::from_static(&[0xFF, 0x80, 0x40, 0xFF]),
            None,
        ))
    }

    /// Record `texture` as decoded in the shared store.
    fn decode(app: &mut App, texture: TextureKey, level: DiscardLevel) {
        let _replaced = app
            .world_mut()
            .resource_mut::<DecodedTextures>()
            .insert(texture, decoded(level));
    }

    /// The image a node ended up showing, if any.
    fn shown(app: &App, node: Entity) -> Option<AssetId<Image>> {
        app.world()
            .get::<ImageNode>(node)
            .map(|image| image.image.id())
    }

    /// Two nodes showing the same texture share one uploaded image — the leak
    /// this replaced uploaded a full RGBA copy per node, per re-open, forever.
    #[test]
    fn one_upload_serves_every_node_showing_the_texture() {
        let mut app = texture_app();
        let texture = key(0xA1);
        decode(&mut app, texture, DiscardLevel::FULL);
        let first = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        let second = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        app.update();

        assert_eq!(
            shown(&app, first),
            shown(&app, second),
            "the two nodes were uploaded their own copy of one texture"
        );
        assert_eq!(
            app.world().resource::<Assets<Image>>().len(),
            1,
            "one texture, more than one image"
        );
        assert_eq!(app.world().resource::<UiTextureImages>().len(), 1);
    }

    /// A texture that re-decodes at a finer level is uploaded again, rather than
    /// every later window being served the coarse image forever.
    #[test]
    fn a_finer_decode_is_uploaded_again() {
        let mut app = texture_app();
        let texture = key(0xB2);
        decode(&mut app, texture, DiscardLevel::MAX);
        let coarse = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        app.update();

        decode(&mut app, texture, DiscardLevel::FULL);
        let fine = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        app.update();

        assert_ne!(
            shown(&app, coarse),
            shown(&app, fine),
            "the finer decode was served the coarse upload"
        );
        assert_eq!(app.world().resource::<UiTextureImages>().len(), 1);
    }

    /// An image nothing shows any more is dropped by Bevy, and the next node
    /// that wants it uploads it again instead of showing an empty box.
    #[test]
    fn a_released_upload_is_rebuilt() -> Result<(), TestError> {
        let mut app = texture_app();
        let texture = key(0xC3);
        decode(&mut app, texture, DiscardLevel::FULL);
        let first = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        app.update();
        let released = shown(&app, first).ok_or("the first node never got its image")?;
        // What the last handle dropping does, without an `AssetPlugin` to
        // notice it: the image is gone, the map's id no longer resolves.
        let _removed = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .remove_untracked(released);

        let second = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        app.update();

        let shown_again = shown(&app, second);
        assert!(
            shown_again.is_some_and(|id| id != released),
            "a released image was handed out again as a dangling id"
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
        Ok(())
    }

    /// The "(loading)" label under an image box goes when the image lands — and
    /// only for the boxes that said they were holding one.
    #[test]
    fn only_a_placeholder_box_drops_its_children() {
        let mut app = texture_app();
        let texture = key(0xD4);
        decode(&mut app, texture, DiscardLevel::FULL);
        let labelled = app
            .world_mut()
            .spawn(PendingUiTexture::over_placeholder(texture))
            .id();
        let label = app.world_mut().spawn(ChildOf(labelled)).id();
        let plain = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        let kept = app.world_mut().spawn(ChildOf(plain)).id();
        app.update();

        assert!(
            app.world().get_entity(label).is_err(),
            "the loading label stayed under the image"
        );
        assert!(
            app.world().get_entity(kept).is_ok(),
            "a box that holds no placeholder had its content despawned"
        );
    }

    /// A waiting node asks the pipeline for its texture, and stops waiting once
    /// the image is in.
    #[test]
    fn a_waiting_node_asks_for_its_texture_then_stops_waiting() {
        let mut app = texture_app();
        let texture = key(0xE5);
        let node = app.world_mut().spawn(PendingUiTexture::new(texture)).id();
        app.update();

        let asked: Vec<BoostTexture> = app
            .world_mut()
            .resource_mut::<Messages<BoostTexture>>()
            .drain()
            .collect();
        assert_eq!(asked.len(), 1, "the texture was not requested exactly once");
        assert_eq!(asked.first().map(|boost| boost.key), Some(texture));
        assert_eq!(
            asked.first().map(|boost| boost.priority),
            Some(AVATAR_BOOST_PRIORITY)
        );
        assert!(
            app.world().get::<PendingUiTexture>(node).is_some(),
            "the node stopped waiting before its texture decoded"
        );
        assert!(shown(&app, node).is_none());

        decode(&mut app, texture, DiscardLevel::FULL);
        app.update();

        assert!(
            app.world().get::<PendingUiTexture>(node).is_none(),
            "the node is still waiting for a texture it has been painted with"
        );
        assert!(shown(&app, node).is_some());
    }

    /// Pointing a box at a second texture asks for that one too — the picker's
    /// preview pane follows the selection through the same node.
    #[test]
    fn a_repointed_box_asks_for_the_new_texture() {
        let mut app = texture_app();
        let (first, second) = (key(0xF6), key(0xF7));
        let node = app.world_mut().spawn(PendingUiTexture::new(first)).id();
        app.update();
        let _first_ask = app
            .world_mut()
            .resource_mut::<Messages<BoostTexture>>()
            .drain()
            .count();

        app.world_mut()
            .entity_mut(node)
            .insert(PendingUiTexture::new(second));
        app.update();

        let asked: Vec<TextureKey> = app
            .world_mut()
            .resource_mut::<Messages<BoostTexture>>()
            .drain()
            .map(|boost| boost.key)
            .collect();
        assert_eq!(asked, vec![second]);
    }
}
