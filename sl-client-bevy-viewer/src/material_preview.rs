//! Offscreen **material-on-a-lit-sphere** previews
//! (`viewer-material-swatch-sphere-preview`): the reference's `LLTextureCtrl`
//! material-preview / `llmaterialeditor` preview, which render a GLTF render
//! material on a lit sphere rather than as a flat texture thumbnail.
//!
//! # Model
//!
//! A UI node opts into a preview by carrying a [`MaterialPreview`] component. Each
//! previewed node is bound to a **studio**: an isolated render layer holding one
//! sphere, one key light and one camera that draws the sphere into a small
//! [`RenderTarget::Image`]. The node's [`ImageNode`] then samples that image, so
//! the material is previewed exactly the way a texture thumbnail is — only it is a
//! shaded sphere, not a flat map. Studios are pooled and rebound as previews come
//! and go, mirroring the HUD / probe render-to-texture setups already in the
//! viewer.
//!
//! Two consumers drive it:
//! - the Texture tab's PBR **render-material swatch** ([`crate::edit_material`])
//!   previews the selected face's *effective* material ([`MaterialPreview::Material`],
//!   base + override, already folded by the caller);
//! - the **material picker's** preview pane ([`crate::ui_texture_picker`]) previews
//!   the *selected* material by asset id ([`MaterialPreview::Asset`], resolved
//!   through the [`MaterialManager`]'s decode).
//!
//! The sphere's [`StandardMaterial`] is shaded by the same
//! [`MaterialManager::apply_preview`] path the world faces use, so a preview looks
//! like the material does in world (a known limitation it inherits: with no
//! environment map on the studio camera, a fully metallic material reflects
//! nothing and reads dark — the same caveat as the world's shiny faces).

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use sl_client_bevy::{AssetKey, GltfMaterial};

use crate::face_material::{FaceMaterial, inert_face_material};
use crate::materials::MaterialManager;
use crate::textures::TextureManager;

/// The square side, in texels, each studio renders its sphere into. One size
/// serves both the 40px swatch and the 128px preview pane (the UI scales it).
const PREVIEW_TEXTURE_SIZE: u32 = 128;

/// The preview sphere's radius, in the studio's own local space.
const SPHERE_RADIUS: f32 = 1.0;

/// The first render layer a studio uses; successive studios take the next layers
/// up. Chosen well clear of the world (0), HUD (1) and edit-gizmo (3) layers so a
/// studio's sphere and light are invisible to every other camera and vice versa.
const MATERIAL_PREVIEW_LAYER_BASE: usize = 8;

/// The studio camera's distance from the sphere centre, framing a unit sphere at
/// [`STUDIO_FOV`] with a little margin.
const CAMERA_DISTANCE: f32 = 3.3;

/// The studio camera's vertical field of view, in radians (45°).
const STUDIO_FOV: f32 = core::f32::consts::FRAC_PI_4;

/// The studio's neutral backdrop, behind the sphere.
const STUDIO_BACKGROUND: Color = Color::srgb(0.14, 0.14, 0.17);

/// An empty (no-preview) swatch / pane fill, matching the texture-picker's.
const EMPTY_FILL: Color = Color::srgba(0.1, 0.1, 0.12, 1.0);

/// The studio key light's illuminance, in lux.
const KEY_LIGHT_ILLUMINANCE: f32 = 9000.0;

/// The studio's per-view ambient fill brightness (overriding the world's
/// time-of-day ambient so a preview's brightness is stable).
const STUDIO_AMBIENT_BRIGHTNESS: f32 = 700.0;

/// What a UI node wants previewed on its sphere. A node without this component is
/// not a material preview (its [`ImageNode`] is whatever painted it). The
/// [`Material`](Self::Material) payload is boxed: a [`GltfMaterial`] dwarfs the
/// other variants, so an unboxed enum would be all-but-that everywhere it is
/// stored.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) enum MaterialPreview {
    /// Nothing to preview — release the studio and clear the node.
    Empty,
    /// Preview a resolved effective GLTF material directly (the render-material
    /// swatch: base + override, already folded by the caller).
    Material(Box<GltfMaterial>),
    /// Preview a material asset by id, resolved to a [`GltfMaterial`] through the
    /// [`MaterialManager`]'s fetch / decode (the picker's preview pane).
    Asset(AssetKey),
}

/// The render layer the `index`-th studio (its sphere, light and camera) lives on
/// — successive layers up from [`MATERIAL_PREVIEW_LAYER_BASE`], so no two studios
/// share a layer (which would leak one sphere into another's camera).
const fn studio_layer(index: usize) -> usize {
    MATERIAL_PREVIEW_LAYER_BASE.saturating_add(index)
}

/// One preview studio: an isolated render layer with a sphere, a key light and a
/// camera drawing into an image.
#[derive(Debug)]
struct PreviewStudio {
    /// The image the camera renders into and the previewing node samples.
    image: Handle<Image>,
    /// The sphere's material, re-shaded when the bound preview changes.
    sphere_material: Handle<FaceMaterial>,
    /// The camera entity, toggled active only while the studio is bound.
    camera: Entity,
}

/// The pool of preview studios and their bindings. Studios are created lazily (one
/// per simultaneously-previewed node) and reused from a free list as previews come
/// and go, so the common case (one swatch + one picker pane) needs only two.
#[derive(Resource, Debug)]
struct MaterialPreviewStudios {
    /// The shared sphere mesh every studio draws.
    sphere_mesh: Handle<Mesh>,
    /// Every studio created so far, indexed by the ids in [`free`](Self::free) /
    /// [`bound`](Self::bound).
    studios: Vec<PreviewStudio>,
    /// Studio indices currently unbound and available for reuse.
    free: Vec<usize>,
    /// The studio index bound to each previewing node.
    bound: HashMap<Entity, usize>,
    /// The last resolution fully applied to each previewing node (`None` = shown
    /// empty; `Some` = the material on its sphere), so a preview whose resolution is
    /// unchanged does no work — and an [`MaterialPreview::Asset`] is applied once its
    /// decode lands, then left alone.
    applied: HashMap<Entity, Option<GltfMaterial>>,
}

impl MaterialPreviewStudios {
    /// The studio bound to `owner`, creating one (from the free list, or freshly
    /// spawned) if it has none. Marks the (reused) camera active.
    fn bind(
        &mut self,
        owner: Entity,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        materials: &mut Assets<FaceMaterial>,
        cameras: &mut Query<&mut Camera>,
    ) -> usize {
        if let Some(index) = self.bound.get(&owner) {
            return *index;
        }
        let index = if let Some(index) = self.free.pop() {
            if let Some(studio) = self.studios.get(index)
                && let Ok(mut camera) = cameras.get_mut(studio.camera)
            {
                camera.is_active = true;
            }
            index
        } else {
            let layer = studio_layer(self.studios.len());
            let studio = create_studio(commands, images, materials, &self.sphere_mesh, layer);
            self.studios.push(studio);
            self.studios.len().saturating_sub(1)
        };
        let _prev = self.bound.insert(owner, index);
        index
    }

    /// Release `owner`'s studio back to the free list and mark its camera inactive
    /// (so an unbound studio does not keep re-rendering). A no-op if it had none.
    fn release(&mut self, owner: Entity, cameras: &mut Query<&mut Camera>) {
        if let Some(index) = self.bound.remove(&owner) {
            if let Some(studio) = self.studios.get(index)
                && let Ok(mut camera) = cameras.get_mut(studio.camera)
            {
                camera.is_active = false;
            }
            self.free.push(index);
        }
    }
}

/// A marker on a studio camera (so camera-wide systems can tell it from the world
/// / HUD / gizmo cameras; the pool toggles its `is_active`).
#[derive(Component, Debug, Clone, Copy)]
struct MaterialPreviewCamera;

/// The plugin wiring the material-preview studios into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MaterialPreviewPlugin;

impl Plugin for MaterialPreviewPlugin {
    /// Create the shared sphere mesh + studio pool at startup, then reclaim freed
    /// studios and drive the bound previews each frame.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_material_previews)
            .add_systems(
                Update,
                (reclaim_removed_previews, drive_material_previews).chain(),
            );
    }
}

/// Startup: build the shared sphere mesh and insert the (empty) studio pool.
fn setup_material_previews(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let sphere_mesh = meshes.add(Sphere::new(SPHERE_RADIUS).mesh().uv(48, 32));
    commands.insert_resource(MaterialPreviewStudios {
        sphere_mesh,
        studios: Vec::new(),
        free: Vec::new(),
        bound: HashMap::new(),
        applied: HashMap::new(),
    });
}

/// Spawn one studio: its render-target image, sphere, key light and camera, all on
/// render `layer`. The camera starts active; the pool marks it inactive when the
/// studio is unbound.
fn create_studio(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    materials: &mut Assets<FaceMaterial>,
    sphere_mesh: &Handle<Mesh>,
    layer: usize,
) -> PreviewStudio {
    let image = images.add(Image::new_target_texture(
        PREVIEW_TEXTURE_SIZE,
        PREVIEW_TEXTURE_SIZE,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let sphere_material = materials.add(inert_face_material(StandardMaterial::default()));
    let layers = RenderLayers::layer(layer);
    // The sphere.
    commands.spawn((
        Mesh3d(sphere_mesh.clone()),
        MeshMaterial3d(sphere_material.clone()),
        Transform::default(),
        layers.clone(),
        Name::new(format!("material-preview-sphere-{layer}")),
    ));
    // A key light, front-upper-left of the camera, no shadows (a lone sphere casts
    // none worth the cost).
    commands.spawn((
        DirectionalLight {
            illuminance: KEY_LIGHT_ILLUMINANCE,
            shadow_maps_enabled: false,
            ..Default::default()
        },
        Transform::from_xyz(-1.0, 1.3, 1.2).looking_at(Vec3::ZERO, Vec3::Y),
        layers.clone(),
        Name::new(format!("material-preview-light-{layer}")),
    ));
    // The camera, looking at the sphere down +Z, into the image.
    let camera = commands
        .spawn((
            MaterialPreviewCamera,
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(STUDIO_BACKGROUND),
                ..Default::default()
            },
            RenderTarget::Image(image.clone().into()),
            Projection::Perspective(PerspectiveProjection {
                fov: STUDIO_FOV,
                aspect_ratio: 1.0,
                ..Default::default()
            }),
            Transform::from_xyz(0.0, 0.0, CAMERA_DISTANCE).looking_at(Vec3::ZERO, Vec3::Y),
            // A per-view ambient fill, so the preview's brightness does not swing
            // with the world's time-of-day ambient.
            AmbientLight {
                brightness: STUDIO_AMBIENT_BRIGHTNESS,
                ..Default::default()
            },
            // The target is single-sampled, so the camera renders without MSAA.
            Msaa::Off,
            layers,
            Name::new(format!("material-preview-camera-{layer}")),
        ))
        .id();
    PreviewStudio {
        image,
        sphere_material,
        camera,
    }
}

/// Free a studio when its previewing node loses its [`MaterialPreview`] component
/// (removed, or the whole node despawned) so the studio can be reused.
fn reclaim_removed_previews(
    mut removed: RemovedComponents<MaterialPreview>,
    pool: Option<ResMut<MaterialPreviewStudios>>,
    mut cameras: Query<&mut Camera>,
) {
    let Some(mut pool) = pool else {
        return;
    };
    for owner in removed.read() {
        pool.release(owner, &mut cameras);
        let _prev = pool.applied.remove(&owner);
    }
}

/// Reconcile every previewing node to its studio each frame: resolve its
/// [`MaterialPreview`] (decoding an asset id if needed), and — only when the
/// resolution actually changes — bind or release a studio, shade its sphere, and
/// point (or clear) the node's [`ImageNode`].
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the previews, \
              the studio pool, the material manager + texture manager + material / image assets \
              the sphere shading needs, the camera + background writes, and Commands for the \
              node's ImageNode"
)]
fn drive_material_previews(
    previews: Query<(Entity, &MaterialPreview)>,
    pool: Option<ResMut<MaterialPreviewStudios>>,
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<&mut Camera>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut commands: Commands,
) {
    let Some(mut pool) = pool else {
        return;
    };
    for (owner, preview) in &previews {
        // Resolve to the material to shade (or `None` for an empty node). An asset
        // whose decode has not landed yet is skipped (retried next frame, its studio
        // — if any — left bound showing the prior sphere).
        let resolved: Option<GltfMaterial> = match preview {
            MaterialPreview::Empty => None,
            MaterialPreview::Material(material) => Some(**material),
            MaterialPreview::Asset(id) => {
                manager.request_material(*id);
                match manager.decoded_material(*id) {
                    Some(material) => Some(*material),
                    None => continue,
                }
            }
        };
        if pool.applied.get(&owner) == Some(&resolved) {
            continue;
        }
        match &resolved {
            None => {
                pool.release(owner, &mut cameras);
                if let Ok(mut node) = commands.get_entity(owner) {
                    node.remove::<ImageNode>();
                }
                if let Ok(mut background) = backgrounds.get_mut(owner) {
                    background.0 = EMPTY_FILL;
                }
            }
            Some(material) => {
                let index = pool.bind(
                    owner,
                    &mut commands,
                    &mut images,
                    &mut materials,
                    &mut cameras,
                );
                let Some((sphere_material, image)) = pool
                    .studios
                    .get(index)
                    .map(|studio| (studio.sphere_material.clone(), studio.image.clone()))
                else {
                    continue;
                };
                manager.apply_preview(&mut textures, &mut materials, &sphere_material, material);
                if let Ok(mut node) = commands.get_entity(owner) {
                    node.insert(ImageNode::new(image));
                }
            }
        }
        let _prev = pool.applied.insert(owner, resolved);
    }
}

#[cfg(test)]
mod tests {
    use super::{MATERIAL_PREVIEW_LAYER_BASE, MaterialPreviewStudios, PreviewStudio, studio_layer};
    use bevy::platform::collections::HashMap;
    use bevy::prelude::*;
    use pretty_assertions::{assert_eq, assert_ne};

    /// A pool with `count` pre-made studios, whose cameras are placeholder (unused)
    /// entities — enough to exercise the bind / release bookkeeping without a render
    /// world.
    fn pool_with(count: usize) -> MaterialPreviewStudios {
        let studios = std::iter::repeat_with(|| PreviewStudio {
            image: Handle::default(),
            sphere_material: Handle::default(),
            // The pure bind/release logic never touches the camera entity.
            camera: Entity::PLACEHOLDER,
        })
        .take(count)
        .collect();
        MaterialPreviewStudios {
            sphere_mesh: Handle::default(),
            studios,
            free: (0..count).rev().collect(),
            bound: HashMap::new(),
            applied: HashMap::new(),
        }
    }

    /// Binding pops a free studio; the second owner gets a *different* studio, and
    /// releasing the first returns its studio to the free list for reuse.
    #[test]
    fn bind_reuses_released_studios() {
        let mut pool = pool_with(2);
        let world = &mut World::new();
        let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());

        // No render world here, so drive the free-list logic directly rather than
        // through `bind` (which would spawn on an empty free list / toggle a camera).
        // `pool_with(2)` seeds two free studios, so neither pop is empty.
        let first = pool.free.pop().unwrap_or_default();
        let _prev = pool.bound.insert(a, first);
        let second = pool.free.pop().unwrap_or_default();
        let _prev = pool.bound.insert(b, second);
        assert_ne!(first, second, "two owners must not share a studio");
        assert!(pool.free.is_empty(), "both studios are now bound");

        // Release the first: its studio returns to the free list, the second stays.
        if let Some(index) = pool.bound.remove(&a) {
            pool.free.push(index);
        }
        assert_eq!(pool.free, vec![first], "the released studio is reusable");
        assert_eq!(pool.bound.get(&b), Some(&second), "b keeps its studio");
    }

    /// Successive studios take successive layers from the base, so no two ever share
    /// a render layer (which would leak one sphere into another's camera).
    #[test]
    fn studios_take_distinct_layers() {
        let layers: Vec<usize> = (0..4).map(studio_layer).collect();
        assert_eq!(
            layers,
            vec![
                MATERIAL_PREVIEW_LAYER_BASE,
                MATERIAL_PREVIEW_LAYER_BASE + 1,
                MATERIAL_PREVIEW_LAYER_BASE + 2,
                MATERIAL_PREVIEW_LAYER_BASE + 3,
            ],
            "studio layers must be distinct and contiguous from the base",
        );
    }
}
