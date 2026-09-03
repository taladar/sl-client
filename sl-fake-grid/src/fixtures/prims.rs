//! Typed prim fixtures: a builder that produces the [`Object`] records the
//! fake grid pushes at an arriving viewer.
//!
//! [`world::box_prim`](crate::world::box_prim) makes an untextured cube and
//! nothing else, because `full_update_block` emits only the **raw byte
//! fields** of an [`Object`] — its `texture_entry`, `extra_params`,
//! `particle_system` and `texture_anim` travel as blobs, and the typed views
//! beside them (`extra`, `texture_animation`, `particles`) are what a *decoder*
//! filled in, never what an encoder reads. A fixture that wants a textured,
//! lit, flexi or mesh prim therefore has to encode those blobs itself.
//!
//! [`PrimFixture`] is that encoder. Every builder method sets the typed value
//! and [`build`](PrimFixture::build) packs the four blobs through `sl-proto`'s
//! own [`encode_texture_entry`], [`encode_extra_params`],
//! [`encode_particle_system`] and [`encode_texture_anim`] — the exact inverses
//! of the client's decoders, so a test asserts the fields it seeded.

use sl_proto::{
    ExtendedMesh, FlexibleData, LightData, LightImage, Object, ObjectExtraParams, ParticleSystem,
    PrimShapeParams, ReflectionProbe, RegionLocalObjectId, RenderMaterialRef, SculptData,
    TextureAnimation, TextureEntry, TextureFace, attachment_state_from_point, encode_extra_params,
    encode_particle_system, encode_texture_anim, encode_texture_entry,
};
use sl_types::key::{AgentKey, InventoryKey, MeshKey, ObjectKey, SculptOrMeshKey, TextureKey};
use sl_types::lsl::{Rotation, Vector};

/// The number of texture faces a fixture describes by default. A legacy box
/// has six; giving every fixture six faces means
/// [`face`](PrimFixture::face) can address any of them without the caller
/// having to grow the entry first, and the run-length encoder collapses
/// identical faces back to one default on the wire anyway.
pub const DEFAULT_FACE_COUNT: usize = 6;

/// The sculpt-type code (`LL_SCULPT_TYPE_MESH`) that makes an `ExtraParams`
/// sculpt block name a **mesh asset** rather than a sculpt texture.
const SCULPT_TYPE_MESH: u8 = 5;

/// `LLExtendedMeshParams::ANIMATED_MESH_ENABLED_FLAG` (`llprimitive.h`): the
/// extended-mesh flag that makes a rigged linkset an **animated object**.
const ANIMATED_MESH_ENABLED_FLAG: u32 = 0x1;

/// The shape kind of a sculpted prim — the low bits of the sculpt-type byte
/// (`LL_SCULPT_TYPE_*`), which say how the viewer stitches the sculpt map's
/// edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SculptKind {
    /// Stitch the map into a sphere (`LL_SCULPT_TYPE_SPHERE`, the kind almost
    /// all sculpt content uses).
    #[default]
    Sphere,
    /// Stitch it into a torus (`LL_SCULPT_TYPE_TORUS`).
    Torus,
    /// Leave the edges open (`LL_SCULPT_TYPE_PLANE`).
    Plane,
    /// Stitch the left and right edges only (`LL_SCULPT_TYPE_CYLINDER`).
    Cylinder,
}

impl SculptKind {
    /// The `LL_SCULPT_TYPE_*` code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Sphere => 1,
            Self::Torus => 2,
            Self::Plane => 3,
            Self::Cylinder => 4,
        }
    }
}

/// How one face of a prim is painted: everything a `TextureEntry` face carries
/// in natural units, so a fixture reads as what a builder would have set in
/// the edit floater.
///
/// [`Default`] is the neutral face — no texture change, opaque white, one
/// un-offset repeat, no glow, no shine, not full-bright — so a fixture names
/// only what it is actually testing.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceStyle {
    /// The face's texture, or `None` to leave whatever the prim's default
    /// texture is (what [`PrimFixture::textured`] set, or the blank plywood).
    pub texture: Option<TextureKey>,
    /// The tint applied to the texture, as RGB. `[255; 3]` is no tint.
    pub color: [u8; 3],
    /// The face's opacity, `0.0` (invisible) to `1.0` (opaque).
    pub alpha: f32,
    /// The glow amount, `0.0` to `1.0`.
    pub glow: f32,
    /// Whether the face is full-bright (unlit).
    pub fullbright: bool,
    /// The shininess code: `0` none, `1` low, `2` medium, `3` high.
    pub shiny: u8,
    /// The bump-map code (`BE_*`); `0` is none.
    pub bump: u8,
    /// The texture repeats `(horizontal, vertical)`.
    pub repeats: [f32; 2],
    /// The texture offsets `(horizontal, vertical)`, each in `-1..1`.
    pub offset: [f32; 2],
    /// The texture rotation, in radians.
    pub rotation: f32,
    /// The legacy (`LLMaterial`) material id applied to the face, if any. The
    /// material itself is served by the region's `RenderMaterials` store — see
    /// [`RegionFixture::materials`](super::RegionFixture::materials).
    pub material: Option<uuid::Uuid>,
    /// Whether media (MOAP) is enabled on the face. The media itself is served
    /// by the object's `ObjectMedia` state — see
    /// [`RegionFixture::media`](super::RegionFixture::media).
    pub media: bool,
}

impl Default for FaceStyle {
    fn default() -> Self {
        Self {
            texture: None,
            color: [255; 3],
            alpha: 1.0,
            glow: 0.0,
            fullbright: false,
            shiny: 0,
            bump: 0,
            repeats: [1.0, 1.0],
            offset: [0.0, 0.0],
            rotation: 0.0,
            material: None,
            media: false,
        }
    }
}

/// The bit of the media/tex-gen byte that marks a face as carrying media.
const MEDIA_ENABLED: u8 = 0x01;

impl FaceStyle {
    /// The style applied to `face`, leaving the texture alone when
    /// [`texture`](Self::texture) is `None`.
    fn apply(&self, face: &mut TextureFace) {
        if let Some(texture) = self.texture {
            face.texture_id = texture;
        }
        let [red, green, blue] = self.color;
        face.color = [red, green, blue, alpha_byte(self.alpha)];
        face.glow = self.glow;
        // The wire byte is `bump | fullbright << 5 | shiny << 6`, exactly as
        // the decoder's `bumpmap()` / `fullbright()` / `shininess()` split it.
        face.bump_shiny_fullbright = (self.bump & 0x1f)
            | (u8::from(self.fullbright) << 5_u8)
            | ((self.shiny & 0x03) << 6_u8);
        face.media_flags = if self.media { MEDIA_ENABLED } else { 0 };
        let [scale_s, scale_t] = self.repeats;
        face.scale_s = scale_s;
        face.scale_t = scale_t;
        let [offset_s, offset_t] = self.offset;
        face.offset_s = offset_s;
        face.offset_t = offset_t;
        face.rotation = self.rotation;
        face.material_id = self.material;
    }
}

/// An opacity in `0..=1` as the wire's alpha byte.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=255 before the cast; no From impl exists"
)]
fn alpha_byte(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A prim under construction: the [`Object`] fields a fixture sets, plus the
/// typed views that [`build`](Self::build) packs into the raw blobs.
///
/// Start from [`boxed`](Self::boxed) (a plain box) and chain only what the
/// fixture is about; everything unnamed stays at the neutral value a stock
/// prim has.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimFixture {
    /// The object being built.
    object: Object,
    /// The per-face texture entry, packed on [`build`](Self::build).
    entry: TextureEntry,
    /// The typed extra parameters, packed on [`build`](Self::build).
    extra: ObjectExtraParams,
    /// The typed particle system, if any.
    particles: Option<ParticleSystem>,
    /// The typed texture animation, if any.
    texture_animation: Option<TextureAnimation>,
}

impl PrimFixture {
    /// A plain box prim of `scale` metres resting at `position`, owned by
    /// `owner`: the shape [`world::box_prim`](crate::world::box_prim) makes,
    /// with a neutral six-face texture entry ready for
    /// [`face`](Self::face) / [`textured`](Self::textured).
    #[must_use]
    pub fn boxed(
        local_id: RegionLocalObjectId,
        full_id: ObjectKey,
        owner: AgentKey,
        position: Vector,
        scale: Vector,
    ) -> Self {
        Self {
            object: crate::world::box_prim(local_id, full_id, owner, position, scale),
            entry: TextureEntry {
                faces: vec![TextureFace::new(blank_texture()); DEFAULT_FACE_COUNT],
            },
            extra: ObjectExtraParams::default(),
            particles: None,
            texture_animation: None,
        }
    }

    /// Replaces the prim's shape parameters (path and profile curves, cuts,
    /// twists, hollow) — how a fixture becomes a sphere, cylinder or tube
    /// rather than a box.
    #[must_use]
    pub const fn shape(mut self, shape: PrimShapeParams) -> Self {
        self.object.shape = shape;
        self
    }

    /// Turns the prim to `rotation`.
    #[must_use]
    pub const fn rotated(mut self, rotation: Rotation) -> Self {
        self.object.motion.rotation = rotation;
        self
    }

    /// Puts `texture` on **every** face — the run-length encoder writes it
    /// once as the entry's default.
    #[must_use]
    pub fn textured(mut self, texture: TextureKey) -> Self {
        for face in &mut self.entry.faces {
            face.texture_id = texture;
        }
        self
    }

    /// Styles one face. A face index past the entry's width is ignored (a
    /// six-face box has no seventh face), so a fixture cannot silently grow a
    /// prim's face count past what its shape has.
    #[must_use]
    pub fn face(mut self, index: usize, style: &FaceStyle) -> Self {
        if let Some(face) = self.entry.faces.get_mut(index) {
            style.apply(face);
        }
        self
    }

    /// Styles every face at once — the shorthand for a prim whose whole
    /// surface is one material.
    #[must_use]
    pub fn faces(mut self, style: &FaceStyle) -> Self {
        for face in &mut self.entry.faces {
            style.apply(face);
        }
        self
    }

    /// Makes the prim a **mesh**: the `ExtraParams` sculpt block names `mesh`
    /// with `LL_SCULPT_TYPE_MESH`, which is how a simulator says "fetch this
    /// asset over `GetMesh2` instead of tessellating the prim shape". The mesh
    /// asset itself must be in the region's asset store.
    ///
    /// A mesh's face count comes from its submeshes, so the entry is narrowed
    /// to `faces` — one for
    /// [`sl_test_assets::mesh::unit_cube_mesh_asset`].
    #[must_use]
    pub fn mesh(mut self, mesh: MeshKey, faces: usize) -> Self {
        self.extra.sculpt = Some(SculptData {
            texture: SculptOrMeshKey::Mesh(mesh),
            sculpt_type: SCULPT_TYPE_MESH,
        });
        self.entry.faces.resize(faces.max(1), neutral_face());
        self
    }

    /// Makes a mesh prim an **animated object** (animesh): the `ExtraParams`
    /// extended-mesh block carries `ANIMATED_MESH_ENABLED_FLAG`, which is how a
    /// simulator says "give this linkset a control avatar of its own and pose
    /// its rigged submeshes from it".
    ///
    /// Only meaningful on a [`mesh`](Self::mesh) prim whose asset carries a
    /// `skin` block — an animesh with nothing rigged to its skeleton has
    /// nothing to move. The animations it plays travel separately, as the
    /// `ObjectAnimation` the region pushes for its
    /// [`SceneFixtures::object_animations`](crate::world::SceneFixtures::object_animations)
    /// entry.
    #[must_use]
    pub const fn animated_mesh(mut self) -> Self {
        self.extra.extended_mesh = Some(ExtendedMesh {
            flags: ANIMATED_MESH_ENABLED_FLAG,
        });
        self
    }

    /// Makes the prim a **sculpty**: the `ExtraParams` sculpt block names the
    /// sculpt map and how its edges stitch. The map itself is an ordinary
    /// texture in the region's asset store.
    ///
    /// A sculpty has exactly one face, so the entry is narrowed to one.
    #[must_use]
    pub fn sculpt(mut self, map: TextureKey, kind: SculptKind) -> Self {
        self.extra.sculpt = Some(SculptData {
            texture: SculptOrMeshKey::Sculpt(map),
            sculpt_type: kind.code(),
        });
        self.entry.faces.resize(1, neutral_face());
        self
    }

    /// Applies a GLTF (PBR) render material to one face — the modern
    /// `ExtraParams` `RenderMaterial` block. The material asset is fetched
    /// over `ViewerAsset`, so it belongs in the region's asset store.
    #[must_use]
    pub fn pbr(mut self, face: u8, material_id: uuid::Uuid) -> Self {
        self.extra
            .render_material
            .retain(|entry| entry.face != face);
        self.extra
            .render_material
            .push(RenderMaterialRef { face, material_id });
        self
    }

    /// Makes the prim a point/spot light.
    #[must_use]
    pub const fn light(mut self, light: LightData) -> Self {
        self.extra.light = Some(light);
        self
    }

    /// Gives the prim's light a projected image (a spotlight's gobo).
    #[must_use]
    pub const fn projector(mut self, image: LightImage) -> Self {
        self.extra.light_image = Some(image);
        self
    }

    /// Makes the prim's path flexible ("flexi").
    #[must_use]
    pub const fn flexi(mut self, flexible: FlexibleData) -> Self {
        self.extra.flexible = Some(flexible);
        self
    }

    /// Makes the prim a reflection probe.
    #[must_use]
    pub const fn reflection_probe(mut self, probe: ReflectionProbe) -> Self {
        self.extra.reflection_probe = Some(probe);
        self
    }

    /// Attaches a particle system to the prim.
    #[must_use]
    pub const fn particles(mut self, system: ParticleSystem) -> Self {
        self.particles = Some(system);
        self
    }

    /// Animates the prim's texture (`llSetTextureAnim`).
    #[must_use]
    pub const fn texture_anim(mut self, animation: TextureAnimation) -> Self {
        self.texture_animation = Some(animation);
        self
    }

    /// Floats `text` above the prim in `color` (RGBA; the alpha is the text's
    /// **opacity**, as `llSetText` takes it — `255` fully opaque).
    ///
    /// The alpha is inverted on the way onto the wire, because that is where the
    /// inversion lives: `ObjectUpdate`'s `TextColor` transmits `255 - opacity`,
    /// so a transmitted `0` is fully opaque and a transmitted `255` is the
    /// scripter's "text set but invisible, revealed later" trick (the reference
    /// viewer's `coloru.mV[3] = 255 - coloru.mV[3]`). A fixture that wrote the
    /// byte straight through would read as opaque and render as nothing —
    /// which is exactly what the catalogue's floating text did until a
    /// full-stack capture found no pixels above the prim.
    #[must_use]
    pub fn hover_text(mut self, text: &str, color: [u8; 4]) -> Self {
        let [red, green, blue, opacity] = color;
        text.clone_into(&mut self.object.text);
        self.object.text_color = [red, green, blue, 255_u8.saturating_sub(opacity)];
        self
    }

    /// Sets the prim's legacy whole-object media URL (the pre-MOAP
    /// `MediaURL` field). Per-face media is the `ObjectMedia` capability plus
    /// [`FaceStyle::media`].
    #[must_use]
    pub fn media_url(mut self, url: url::Url) -> Self {
        self.object.media_url = Some(url);
        self
    }

    /// Makes the prim a child of `parent` at `offset` metres and `rotation`,
    /// both **root-relative** — which is how a linkset's child prims travel on
    /// the wire.
    #[must_use]
    pub const fn child_of(
        mut self,
        parent: RegionLocalObjectId,
        offset: Vector,
        rotation: Rotation,
    ) -> Self {
        self.object.parent_id = parent;
        self.object.motion.position = offset;
        self.object.motion.rotation = rotation;
        self
    }

    /// Makes the prim an **attachment** worn by the avatar object `wearer` on
    /// `point` (an `AttachmentPoint` code), carrying the inventory item id the
    /// viewer keys worn items on.
    ///
    /// The attachment point rides in the object's `state` byte with its
    /// nibbles swapped ([`attachment_state_from_point`]) and the item id in
    /// the `AttachItemID` name-value, exactly as a simulator sends it; the
    /// position and rotation are relative to the attachment point.
    #[must_use]
    pub fn attached_to(
        mut self,
        wearer: RegionLocalObjectId,
        point: u8,
        item: InventoryKey,
        offset: Vector,
        rotation: Rotation,
    ) -> Self {
        self.object.parent_id = wearer;
        self.object.state = attachment_state_from_point(point);
        self.object.motion.position = offset;
        self.object.motion.rotation = rotation;
        let attach = format!("AttachItemID STRING RW SV {}", item.uuid());
        if self.object.name_value.is_empty() {
            self.object.name_value = attach;
        } else {
            self.object.name_value = format!("{}\n{attach}", self.object.name_value);
        }
        self
    }

    /// The region-local id this fixture will be rezzed with (what a linkset or
    /// an attachment names as its parent).
    #[must_use]
    pub const fn local_id(&self) -> RegionLocalObjectId {
        self.object.local_id
    }

    /// The finished [`Object`]: every typed value packed into the raw wire
    /// blob beside it, so `full_update_block` — which copies the blobs
    /// verbatim — sends what the builder described.
    #[must_use]
    pub fn build(self) -> Object {
        let Self {
            mut object,
            entry,
            extra,
            particles,
            texture_animation,
        } = self;
        object.texture_entry = encode_texture_entry(&entry);
        object.extra_params = encode_extra_params(&extra);
        object.extra = extra;
        object.particle_system = particles
            .as_ref()
            .map(encode_particle_system)
            .unwrap_or_default();
        object.particles = particles;
        object.texture_anim = texture_animation
            .as_ref()
            .map(encode_texture_anim)
            .unwrap_or_default();
        object.texture_animation = texture_animation;
        object
    }
}

/// A neutral face showing the blank texture.
fn neutral_face() -> TextureFace {
    TextureFace::new(blank_texture())
}

/// The blank-plywood texture id every fresh prim wears
/// ([`sl_proto::DEFAULT_PRIM_TEXTURE`]), so an untextured fixture face is what
/// a freshly rezzed prim looks like rather than a nil-texture hole. The stock
/// asset store answers it ([`default_assets`](crate::scenario::default_assets)).
#[must_use]
pub fn blank_texture() -> TextureKey {
    TextureKey::from(sl_proto::DEFAULT_PRIM_TEXTURE)
}

/// The objects of a **linkset**: the root followed by its children, each
/// child re-parented to the root's region-local id. A child's position and
/// rotation are left as the builder set them, which for a linkset means
/// root-relative — [`PrimFixture::child_of`] sets both at once.
///
/// A linkset is one object on the wire per prim; nothing but the shared
/// `parent_id` links them, which is why this is a function over built objects
/// rather than a container type.
#[must_use]
pub fn linkset(root: PrimFixture, children: Vec<PrimFixture>) -> Vec<Object> {
    let root_id = root.local_id();
    let mut objects = vec![root.build()];
    for child in children {
        let mut object = child.build();
        object.parent_id = root_id;
        objects.push(object);
    }
    objects
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{
        decode_extra_params, decode_particle_system, decode_texture_anim, decode_texture_entry,
        texture_anim_mode,
    };

    use super::*;

    /// The zero vector.
    const ZERO: Vector = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// The identity rotation.
    const NO_ROTATION: Rotation = Rotation {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        s: 1.0,
    };

    /// A fixed owner for the fixtures under test.
    fn owner() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(0x0A))
    }

    /// A fixed object key.
    fn key(id: u128) -> ObjectKey {
        ObjectKey::from(uuid::Uuid::from_u128(id))
    }

    /// A one-metre fixture prim at the origin.
    fn prim(local_id: u32) -> PrimFixture {
        PrimFixture::boxed(
            RegionLocalObjectId(local_id),
            key(u128::from(local_id)),
            owner(),
            ZERO,
            Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        )
    }

    /// Every per-face value the builder sets survives the pack and comes back
    /// out of the client's own decoder — the encoder is the decoder's inverse,
    /// not a lookalike.
    #[test]
    fn a_styled_face_round_trips_through_the_texture_entry() {
        let texture = TextureKey::from(uuid::Uuid::from_u128(0xBEEF));
        let material = uuid::Uuid::from_u128(0xF00D);
        let object = prim(1)
            .textured(blank_texture())
            .face(
                2,
                &FaceStyle {
                    texture: Some(texture),
                    color: [10, 20, 30],
                    alpha: 0.5,
                    glow: 0.25,
                    fullbright: true,
                    shiny: 2,
                    bump: 3,
                    repeats: [4.0, 0.5],
                    offset: [0.25, -0.25],
                    rotation: 1.0,
                    material: Some(material),
                    media: true,
                },
            )
            .build();
        let decoded = decode_texture_entry(&object.texture_entry, DEFAULT_FACE_COUNT);
        let face = decoded.face(2).copied().unwrap_or_else(neutral_face);
        assert_eq!(face.texture_id, texture);
        assert_eq!(face.color, [10, 20, 30, 128]);
        assert!((face.glow - 0.25).abs() < 0.01, "glow {}", face.glow);
        assert!(face.fullbright());
        assert_eq!(face.shininess(), 2);
        assert_eq!(face.bumpmap(), 3);
        assert!(face.media_enabled());
        assert_eq!((face.scale_s, face.scale_t), (4.0, 0.5));
        assert!((face.offset_s - 0.25).abs() < 0.001);
        assert!((face.offset_t + 0.25).abs() < 0.001);
        assert!((face.rotation - 1.0).abs() < 0.001);
        assert_eq!(face.material_id, Some(material));
        // The faces the style did not touch still wear the prim's texture.
        assert_eq!(decoded.texture_id(0), Some(blank_texture()));
    }

    /// A mesh prim names its asset as a mesh, not a sculpt texture, and keeps
    /// exactly the face count its submeshes have.
    #[test]
    fn a_mesh_prim_names_its_asset_and_face_count() {
        let mesh = MeshKey::from(uuid::Uuid::from_u128(0x1D));
        let red = FaceStyle {
            color: [255, 0, 0],
            ..FaceStyle::default()
        };
        let object = prim(2).mesh(mesh, 1).face(3, &red).build();
        let extra = decode_extra_params(&object.extra_params);
        assert_eq!(
            extra
                .sculpt
                .map(|sculpt| (sculpt.texture, sculpt.sculpt_type)),
            Some((SculptOrMeshKey::Mesh(mesh), SCULPT_TYPE_MESH))
        );
        // The entry is one face wide, so face 3 is not a face this prim has
        // and styling it changed nothing. (A blob's face *count* is the
        // reader's, not the writer's — `decode_texture_entry` fills as many
        // faces as it is asked for — so the narrowing shows as the tint
        // never reaching the wire.)
        let one_face = decode_texture_entry(&object.texture_entry, 1);
        assert_eq!(one_face.face(0).map(|face| face.color), Some([255; 4]));
        // The same style on a six-face box does reach face 3.
        let box_prim = prim(2).face(3, &red).build();
        let six = decode_texture_entry(&box_prim.texture_entry, DEFAULT_FACE_COUNT);
        assert_eq!(six.face(3).map(|face| face.color), Some([255, 0, 0, 255]));
    }

    /// A sculpty names its map as a sculpt texture with the stitch kind.
    #[test]
    fn a_sculpt_prim_names_its_map_and_stitch() {
        let map = TextureKey::from(uuid::Uuid::from_u128(0x5C1));
        let object = prim(3).sculpt(map, SculptKind::Torus).build();
        let extra = decode_extra_params(&object.extra_params);
        assert_eq!(
            extra
                .sculpt
                .map(|sculpt| (sculpt.texture, sculpt.sculpt_type)),
            Some((SculptOrMeshKey::Sculpt(map), SculptKind::Torus.code()))
        );
    }

    /// Light, flexi, probe and PBR blocks all reach the wire together: the
    /// `ExtraParams` container carries every present sub-block.
    #[test]
    fn the_extra_params_container_carries_every_block() {
        let material = uuid::Uuid::from_u128(0x9AB);
        let object = prim(4)
            .light(LightData {
                color: [255, 200, 100, 255],
                radius: 8.0,
                cutoff: 0.0,
                falloff: 0.5,
            })
            .flexi(FlexibleData {
                softness: 2,
                tension: 1.0,
                air_friction: 2.0,
                gravity: 0.3,
                wind_sensitivity: 0.5,
                user_force: ZERO,
            })
            .reflection_probe(ReflectionProbe {
                ambiance: 0.5,
                clip_distance: 2.0,
                flags: sl_wire::ReflectionProbeFlags::default(),
            })
            .pbr(1, material)
            .build();
        let extra = decode_extra_params(&object.extra_params);
        assert_eq!(extra.light.map(|light| light.radius), Some(8.0));
        assert_eq!(extra.flexible.as_ref().map(|flexi| flexi.softness), Some(2));
        assert_eq!(
            extra.reflection_probe.map(|probe| probe.ambiance),
            Some(0.5)
        );
        assert_eq!(
            extra.render_material,
            vec![RenderMaterialRef {
                face: 1,
                material_id: material
            }]
        );
        // The typed view beside the blob is what the builder was given; the
        // blob is what the wire can carry, and the flexi block quantizes its
        // floats to a byte each, so the two agree only to that resolution.
        assert_eq!(object.extra.light, extra.light);
        assert_eq!(object.extra.render_material, extra.render_material);
        let gravity = extra.flexible.as_ref().map(|flexi| flexi.gravity);
        assert!(
            gravity.is_some_and(|value| (value - 0.3).abs() < 0.01),
            "the flexi gravity came back as {gravity:?}"
        );
    }

    /// The particle and texture-animation blobs decode back to what was set.
    #[test]
    fn particles_and_texture_animation_round_trip() {
        let system = ParticleSystem {
            crc: 1,
            burst_part_count: 7,
            part_max_age: 3.0,
            part_start_color: [255, 0, 0, 255],
            part_end_color: [0, 0, 255, 0],
            ..sample_particles()
        };
        let animation = TextureAnimation {
            mode: texture_anim_mode::ON | texture_anim_mode::LOOP,
            face: -1,
            size_x: 4,
            size_y: 2,
            start: 0.0,
            length: 8.0,
            rate: 4.0,
        };
        let object = prim(5)
            .particles(system.clone())
            .texture_anim(animation)
            .build();
        assert_eq!(
            decode_particle_system(&object.particle_system).map(|decoded| (
                decoded.burst_part_count,
                decoded.part_start_color,
                decoded.part_end_color
            )),
            Some((7, [255, 0, 0, 255], [0, 0, 255, 0]))
        );
        assert_eq!(decode_texture_anim(&object.texture_anim), Some(animation));
        assert_eq!(object.particles, Some(system));
        assert_eq!(object.texture_animation, Some(animation));
    }

    /// An attachment carries the swizzled point in `state` and the item id in
    /// its name-values, which is what the client reads back.
    #[test]
    fn an_attachment_names_its_point_and_item() {
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x17E));
        let object = prim(6)
            .attached_to(
                RegionLocalObjectId(9),
                6,
                item,
                Vector {
                    x: 0.1,
                    y: 0.0,
                    z: 0.0,
                },
                NO_ROTATION,
            )
            .build();
        assert_eq!(object.parent_id, RegionLocalObjectId(9));
        assert_eq!(object.attachment_point_id(), Some(6));
        assert!(
            object
                .name_values()
                .iter()
                .any(|pair| pair.name == "AttachItemID" && pair.value == item.uuid().to_string()),
            "no AttachItemID in {:?}",
            object.name_value
        );
    }

    /// A linkset re-parents every child to the root, whatever the children
    /// were built with.
    /// Floating text is authored as an **opacity** and transmitted as its
    /// inverse, because that is what the wire carries and what the client
    /// un-inverts. A fixture that wrote the byte straight through would ask for
    /// opaque white and get text nothing draws.
    #[test]
    fn floating_text_transmits_the_inverse_of_its_opacity() {
        let opaque = prim(30)
            .hover_text("catalogue", [255, 255, 255, 255])
            .build();
        assert_eq!(opaque.text, "catalogue");
        assert_eq!(opaque.text_color, [255, 255, 255, 0]);
        // The scripter's invisible-text trick, stated as what it is.
        let invisible = prim(31).hover_text("hidden", [255, 255, 255, 0]).build();
        assert_eq!(invisible.text_color, [255, 255, 255, 255]);
        // A prim with no floating text carries neither the string nor a colour.
        let plain = prim(32).build();
        assert!(plain.text.is_empty());
        assert_eq!(plain.text_color, [0; 4]);
    }

    #[test]
    fn a_linkset_parents_its_children_to_its_root() {
        let root = prim(20);
        let children = vec![
            prim(21).child_of(
                RegionLocalObjectId(0),
                Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                NO_ROTATION,
            ),
            prim(22),
        ];
        let objects = linkset(root, children);
        assert_eq!(objects.len(), 3);
        let parents: Vec<RegionLocalObjectId> =
            objects.iter().map(|object| object.parent_id).collect();
        assert_eq!(
            parents,
            vec![
                RegionLocalObjectId(0),
                RegionLocalObjectId(20),
                RegionLocalObjectId(20)
            ]
        );
    }

    /// A neutral particle system, so the round-trip test names only the
    /// fields it asserts.
    fn sample_particles() -> ParticleSystem {
        ParticleSystem {
            crc: 1,
            flags: 0,
            pattern: sl_proto::particle_pattern::EXPLODE,
            max_age: 0.0,
            start_age: 0.0,
            inner_angle: 0.0,
            outer_angle: 0.0,
            burst_rate: 0.5,
            burst_radius: 1.0,
            burst_speed_min: 0.5,
            burst_speed_max: 1.5,
            burst_part_count: 4,
            angular_velocity: ZERO,
            acceleration: ZERO,
            texture_id: None,
            target_id: None,
            part_flags: 0,
            part_max_age: 2.0,
            part_start_color: [255; 4],
            part_end_color: [255; 4],
            part_start_scale: [0.2, 0.2],
            part_end_scale: [0.1, 0.1],
            part_start_glow: 0.0,
            part_end_glow: 0.0,
            part_blend_func_source: 0,
            part_blend_func_dest: 0,
        }
    }
}
