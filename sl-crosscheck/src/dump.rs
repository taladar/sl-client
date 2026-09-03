//! Reading a `scene.json` — **either viewer's**.
//!
//! The schema is one document with two writers: this workspace's
//! `sl-viewer-world-view::scene_dump` and the patched Firestorm's
//! `fstestscenedump.cpp`. That is why the reader lives here rather than beside
//! either writer: only one of the two is Rust, so no shared struct could cover
//! both, and a reader that could parse only our own dialect could never diff a
//! pair.
//!
//! # It is deliberately lenient, and that is not sloppiness
//!
//! Three kinds of leniency, each paid for by something the reference actually
//! does:
//!
//! - **A missing key is `None`, never an error.** The reference emits `is_mesh`,
//!   `lod` and friends only for an `LLVOVolume`, `mesh_id` only for a mesh,
//!   `loop_time` only when it knows a motion's duration, and `origin_region`
//!   only when the agent has a region. This viewer emits `day_position`,
//!   `drawn_position`, `drawn_rotation` and `has_body`, which the reference does
//!   not emit at all. A reader that required either side's full set would reject
//!   the other's document outright.
//! - **A boolean may arrive as a number.** `face["fullbright"] = (S32)…` on that
//!   side against a JSON `true` on ours: the same fact in two spellings, and the
//!   comparison must see one fact.
//! - **An unknown key is ignored.** The reference reports `visual_complexity`,
//!   `is_fully_loaded`, `camera_mode`, `region_width`, `gltf_material`,
//!   `visible_drawables` and more that this viewer has no counterpart for.
//!   Refusing them would make every dump unreadable the day either viewer adds a
//!   field; they are read where they are comparable and skipped where they are
//!   not.
//!
//! Numbers are read as `f64` throughout even though both viewers write `f32`:
//! the arithmetic in a comparison is done once, in the widest type, rather than
//! rounded on the way in.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A point in Second Life region-local metres, `[x, y, z]`, Z up.
pub type Point = [f64; 3];

/// A quaternion as both viewers emit one: `[x, y, z, w]`.
pub type Quaternion = [f64; 4];

/// What went wrong reading a dump.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: String,
        /// Why not.
        source: std::io::Error,
    },
    /// The file is not the document this reader knows.
    #[error("could not parse {path} as a scene dump: {source}")]
    Parse {
        /// The file that could not be parsed.
        path: String,
        /// Why not.
        source: serde_json::Error,
    },
}

/// A whole scene dump, as either viewer writes it.
#[expect(
    clippy::module_name_repetitions,
    reason = "both viewers call the document a scene dump — the reference's own writer is \
              `fstestscenedump.cpp` — and the comparison is easier to follow when the type is \
              spelled the way the file is talked about"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneDump {
    /// The schema the document claims to be written to. Compared before
    /// anything else: two documents at different versions are not comparable,
    /// and saying so beats a page of spurious differences.
    #[serde(default)]
    pub schema_version: Option<u32>,
    /// What produced it, and where.
    #[serde(default)]
    pub context: Context,
    /// The framing of the shot.
    #[serde(default)]
    pub camera: Camera,
    /// The lighting the frame was rendered under.
    #[serde(default)]
    pub environment: Environment,
    /// The render settings that decide what the frame could contain at all.
    #[serde(default)]
    pub render: Render,
    /// Every object in the agent's own region.
    #[serde(default)]
    pub objects: Vec<Object>,
    /// Every avatar the viewer was showing.
    #[serde(default)]
    pub avatars: Vec<Avatar>,
}

impl SceneDump {
    /// Read a dump from a file.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] when the file cannot be read, [`Error::Parse`] when it
    /// is not this document. Both name the path: a report reads two
    /// dumps and "could not parse it" is useless when it does not say which.
    pub fn read(path: &Path) -> Result<Self, Error> {
        let text = fs_err::read_to_string(path).map_err(|source| Error::Read {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| Error::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    /// Which viewer wrote this, as it names itself — `sl-client` or
    /// `firestorm`.
    #[must_use]
    pub fn viewer(&self) -> &str {
        self.context.viewer.as_deref().unwrap_or("unknown")
    }
}

/// Identity and build of the viewer, and where it was.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    /// Which viewer wrote this.
    #[serde(default)]
    pub viewer: Option<String>,
    /// Its channel.
    #[serde(default)]
    pub channel: Option<String>,
    /// Its version.
    #[serde(default)]
    pub version: Option<String>,
    /// When the dump was taken.
    #[serde(default)]
    pub time: Option<String>,
    /// The grid, as the viewer's own grid manager names it.
    #[serde(default)]
    pub grid: Option<String>,
    /// The agent's region.
    #[serde(default)]
    pub region_name: Option<String>,
    /// Its id.
    #[serde(default)]
    pub region_id: Option<String>,
    /// Its handle, as a string on both sides.
    #[serde(default)]
    pub region_handle: Option<String>,
}

/// Where the camera was and what it could see.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// The eye, in region metres.
    #[serde(default)]
    pub origin_region: Option<Point>,
    /// What it looks at, in region metres.
    #[serde(default)]
    pub focus_region: Option<Point>,
    /// The view axis.
    #[serde(default)]
    pub at_axis: Option<Point>,
    /// The camera's up axis.
    #[serde(default)]
    pub up_axis: Option<Point>,
    /// The camera's left axis.
    #[serde(default)]
    pub left_axis: Option<Point>,
    /// The vertical field of view, in radians.
    #[serde(default)]
    pub fov_radians: Option<f64>,
    /// The frame's aspect ratio.
    #[serde(default)]
    pub aspect: Option<f64>,
    /// The near clip plane, in metres.
    #[serde(default)]
    pub near_clip: Option<f64>,
    /// The far clip plane, in metres.
    #[serde(default)]
    pub far_clip: Option<f64>,
}

/// The sky the frame was lit by.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    /// Where in the day cycle the frame was rendered. This viewer only.
    #[serde(default)]
    pub day_position: Option<f64>,
    /// The sun's direction.
    #[serde(default)]
    pub sun_direction: Option<Point>,
    /// The moon's direction.
    #[serde(default)]
    pub moon_direction: Option<Point>,
    /// The sun's orientation.
    #[serde(default)]
    pub sun_rotation: Option<Quaternion>,
    /// The name of the sky frame in force.
    #[serde(default)]
    pub sky_name: Option<String>,
    /// The name of the water settings in force.
    #[serde(default)]
    pub water_name: Option<String>,
}

/// The render settings in force.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Render {
    /// `RenderFarClip`: how far the simulator streams content toward the agent.
    #[serde(default)]
    pub draw_distance: Option<f64>,
    /// `RenderQualityPerformance`: the graphics preset.
    #[serde(default)]
    pub quality_level: Option<i64>,
    /// `RenderShadowDetail`.
    #[serde(default)]
    pub shadow_detail: Option<i64>,
    /// `RenderVolumeLODFactor`: the mesh / prim level-of-detail multiplier.
    #[serde(default)]
    pub mesh_lod_boost: Option<f64>,
    /// `RenderMaxTextureResolution`.
    #[serde(default)]
    pub max_texture_res: Option<i64>,
    /// `RenderReflectionProbeDetail`.
    #[serde(default)]
    pub reflection_detail: Option<i64>,
}

/// One in-world object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Object {
    /// The object's grid-wide id.
    #[serde(default)]
    pub id: String,
    /// Its region-local id. Read as a signed number because the reference casts
    /// a `U32` through `S32` on the way out, so a local id past `i32::MAX`
    /// arrives negative there and positive here — the same 32 bits, and
    /// [`local_id`](Self::local_id) is what makes them one number again.
    #[serde(default)]
    pub local_id: Option<i64>,
    /// Its class, in the reference's spelling.
    #[serde(default)]
    pub pcode: Option<String>,
    /// Where it is, in region metres.
    #[serde(default)]
    pub position: Option<Point>,
    /// How it is turned.
    #[serde(default)]
    pub rotation: Option<Quaternion>,
    /// Where a worn object was actually drawn. This viewer only.
    #[serde(default)]
    pub drawn_position: Option<Point>,
    /// How a worn object was actually drawn. This viewer only.
    #[serde(default)]
    pub drawn_rotation: Option<Quaternion>,
    /// Its size in metres.
    #[serde(default)]
    pub scale: Option<Point>,
    /// The face count — drawn here, declared there.
    #[serde(default)]
    pub num_faces: Option<i64>,
    /// Those faces.
    #[serde(default)]
    pub faces: Vec<Face>,
    /// Whether it is being drawn.
    #[serde(default, deserialize_with = "loose_bool")]
    pub visible: Option<bool>,
    /// Whether its shape comes from a mesh asset.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_mesh: Option<bool>,
    /// That asset, when it does.
    #[serde(default)]
    pub mesh_id: Option<String>,
    /// Whether its shape comes from a sculpt map.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_sculpt: Option<bool>,
    /// The level of detail it is tessellated at.
    #[serde(default)]
    pub lod: Option<i64>,
    /// Flexible: declared here, being drawn there.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_flexible: Option<bool>,
    /// Whether it emits light.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_light: Option<bool>,
}

impl Object {
    /// The region-local id as the 32 bits both viewers hold, whichever sign the
    /// document wrote them with.
    #[must_use]
    pub fn local_id(&self) -> Option<u32> {
        self.local_id
            .map(|raw| u32::try_from(raw & 0xffff_ffff).unwrap_or(u32::MAX))
    }

    /// Whether this is a **viewer-side scene object** — something the viewer
    /// built for itself rather than something the grid sent.
    ///
    /// The reference models its terrain patches, sky, water and clouds as
    /// objects: `LL_PCODE_APP` classes (`app-30` and friends), `local_id` 0, and
    /// a freshly minted id every run. This viewer does not model them as objects
    /// at all, so 275 of the reference's 296 objects in the catalogue scene are
    /// these. They are neither missing nor different; they are scenery, and a
    /// comparison that reports them as absent buries everything else.
    #[must_use]
    pub fn is_viewer_scenery(&self) -> bool {
        self.local_id() == Some(0)
            && self
                .pcode
                .as_deref()
                .is_some_and(|pcode| pcode.starts_with("app"))
    }
}

/// One face of an object — where "the texture is wrong" actually lives.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// The face index.
    #[serde(default)]
    pub index: Option<i64>,
    /// Its texture asset id.
    #[serde(default)]
    pub texture: Option<String>,
    /// Its tint, RGBA in `0.0..=1.0`.
    #[serde(default)]
    pub color: Option<[f64; 4]>,
    /// Horizontal repeats.
    #[serde(default)]
    pub scale_s: Option<f64>,
    /// Vertical repeats.
    #[serde(default)]
    pub scale_t: Option<f64>,
    /// Horizontal offset.
    #[serde(default)]
    pub offset_s: Option<f64>,
    /// Vertical offset.
    #[serde(default)]
    pub offset_t: Option<f64>,
    /// Texture rotation, in radians.
    #[serde(default)]
    pub rotation: Option<f64>,
    /// The bump-map code.
    #[serde(default)]
    pub bump: Option<i64>,
    /// The shininess code.
    #[serde(default)]
    pub shiny: Option<i64>,
    /// Whether the face is unlit. A JSON boolean here, an integer there.
    #[serde(default, deserialize_with = "loose_bool")]
    pub fullbright: Option<bool>,
    /// The glow amount.
    #[serde(default)]
    pub glow: Option<f64>,
    /// A legacy Blinn-Phong material, when the face has one.
    #[serde(default)]
    pub material_id: Option<String>,
}

/// One avatar.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Avatar {
    /// The agent's id — or, for a control avatar in **this viewer's** dump, the
    /// animesh object's key. See [`is_control_avatar`](Self::is_control_avatar).
    #[serde(default)]
    pub id: String,
    /// Whether this is the logged-in agent.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_self: Option<bool>,
    /// Where the avatar is: the position of its **object**, on both sides.
    #[serde(default)]
    pub position: Option<Point>,
    /// How it is turned.
    #[serde(default)]
    pub rotation: Option<Quaternion>,
    /// Where this viewer drew the body root. This viewer only.
    #[serde(default)]
    pub drawn_position: Option<Point>,
    /// How the drawn body root was turned. This viewer only.
    #[serde(default)]
    pub drawn_rotation: Option<Quaternion>,
    /// Whether this is an animesh's control avatar rather than a resident.
    ///
    /// A control avatar has no grid identity: the reference mints a local UUID
    /// for it and this viewer reports the animesh object's key, so the two ids
    /// never match and matching them is a mistake rather than a finding.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_control_avatar: Option<bool>,
    /// What it is playing, in the order the viewer applies it.
    #[serde(default)]
    pub animations: Vec<Animation>,
    /// Whether this viewer drew it as a body rather than a placeholder. This
    /// viewer only; the reference's nearest is `is_fully_loaded`.
    #[serde(default, deserialize_with = "loose_bool")]
    pub has_body: Option<bool>,
    /// The reference's own "is it fully loaded", read so a report can put the
    /// two side by side without claiming they measure the same thing.
    #[serde(default, deserialize_with = "loose_bool")]
    pub is_fully_loaded: Option<bool>,
}

/// One animation an avatar is playing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// The animation's asset id.
    #[serde(default)]
    pub id: String,
    /// The simulator's per-avatar sequence number, when the simulator asked for
    /// this animation.
    #[serde(default)]
    pub sequence: Option<i64>,
    /// Seconds since **this viewer** started playing it, which is not a number
    /// the two sides can be compared on.
    #[serde(default)]
    pub time: Option<f64>,
    /// Where in the motion that lands — the frame of the animation the body was
    /// drawn at, and the number that *is* comparable.
    #[serde(default)]
    pub loop_time: Option<f64>,
    /// The motion's length in seconds.
    #[serde(default)]
    pub duration: Option<f64>,
    /// Whether it loops.
    #[serde(default, deserialize_with = "loose_bool")]
    pub looping: Option<bool>,
    /// Its base priority.
    #[serde(default)]
    pub priority: Option<i64>,
    /// Whether it has been stopped and is easing out.
    #[serde(default, deserialize_with = "loose_bool")]
    pub stopping: Option<bool>,
}

/// Read a boolean that may have been written as a number.
///
/// `face["fullbright"] = (S32)te->getFullbright()` on the reference's side
/// against a JSON `true` on ours. Both mean "this face is unlit", and a reader
/// that saw two types would report a divergence on every unlit face in every
/// scene.
fn loose_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    /// Accepts either spelling and nothing else.
    struct Loose;

    impl<'de> serde::de::Visitor<'de> for Loose {
        type Value = Option<bool>;

        fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            formatter.write_str("a boolean, or the integer a C++ (S32) cast writes one as")
        }

        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v != 0))
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v != 0))
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }
    }

    deserializer.deserialize_option(Loose)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::SceneDump;

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A fragment of the reference's dialect, spelled as `fstestscenedump.cpp`
    /// spells it: integers where this viewer writes booleans, keys this viewer
    /// has no counterpart for, and none of the keys this viewer adds.
    const FIRESTORM: &str = r#"{
      "schema_version": 1,
      "context": { "viewer": "firestorm", "channel": "Firestorm-Test",
                   "version": "7.1.11", "grid": "127.0.0.1:9100",
                   "region_name": "Fixture", "region_width": 256 },
      "camera": { "origin_region": [128.0, 120.0, 25.0], "fov_radians": 1.0472,
                  "aspect": 1.388, "camera_mode": 1 },
      "environment": { "sun_direction": [0.0, 0.0, 1.0], "sky_name": "Default",
                       "selected": 0 },
      "render": { "draw_distance": 128.0, "mesh_lod_boost": 2.0,
                  "visible_drawables": 4210 },
      "objects": [
        { "id": "1c0de1f0-0000-0000-0000-00000000000a", "local_id": 42,
          "pcode": "volume", "position": [128.0, 128.0, 24.0],
          "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [0.5, 0.5, 0.5],
          "num_faces": 6, "visible": true, "is_mesh": false, "lod": 3,
          "faces": [ { "index": 0, "texture": "89556747-24cb-43ed-920b-47caed15465f",
                       "color": [1.0, 1.0, 1.0, 1.0], "fullbright": 0,
                       "bump": 0, "shiny": 0, "glow": 0.0 } ] },
        { "id": "8f3a0000-0000-0000-0000-000000000001", "local_id": 0,
          "pcode": "app-30", "position": [0.0, 0.0, 0.0], "num_faces": 0,
          "faces": [] }
      ],
      "avatars": [
        { "id": "5b1f0000-0000-0000-0000-000000000002", "is_self": true,
          "position": [128.0, 120.0, 23.0], "rotation": [0.0, 0.0, 0.0, 1.0],
          "is_control_avatar": false, "is_fully_loaded": true,
          "visual_complexity": 12345,
          "animations": [ { "id": "e6e8d1dd-e643-fff7-b238-c6b4b056a68d",
                            "time": 12.5, "looping": true, "stopping": false } ] }
      ]
    }"#;

    /// A fragment of this viewer's dialect: booleans as booleans, and the four
    /// keys the reference does not write.
    const SL_CLIENT: &str = r#"{
      "schema_version": 1,
      "context": { "viewer": "sl-client", "channel": "sl-client",
                   "version": "0.1.0", "grid": "127.0.0.1:9100" },
      "camera": { "origin_region": [128.0, 120.0, 25.0], "fov_radians": 1.0472,
                  "aspect": 1.778 },
      "environment": { "day_position": 0.25, "sun_direction": [0.0, 0.0, 1.0],
                       "sky_name": "Default" },
      "render": { "draw_distance": 512.0, "mesh_lod_boost": 1.0 },
      "objects": [
        { "id": "1c0de1f0-0000-0000-0000-00000000000a", "local_id": 42,
          "pcode": "volume", "position": [128.0, 128.0, 24.0],
          "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [0.5, 0.5, 0.5],
          "num_faces": 6, "visible": true, "is_mesh": false, "is_sculpt": false,
          "lod": 3, "is_flexible": false, "is_light": false,
          "faces": [ { "index": 0, "texture": "89556747-24cb-43ed-920b-47caed15465f",
                       "color": [1.0, 1.0, 1.0, 1.0], "fullbright": false,
                       "bump": 0, "shiny": 0, "glow": 0.0 } ] }
      ],
      "avatars": [
        { "id": "5b1f0000-0000-0000-0000-000000000002", "is_self": true,
          "position": [128.0, 120.0, 23.0], "rotation": [0.0, 0.0, 0.0, 1.0],
          "drawn_position": [128.0, 120.0, 22.06], "is_control_avatar": false,
          "has_body": true, "animations": [] }
      ]
    }"#;

    /// The reference's document reads, integers-for-booleans and all, and the
    /// keys this viewer does not write come back as absent rather than as an
    /// error.
    #[test]
    fn the_references_dialect_reads() -> Result<(), TestError> {
        let dump: SceneDump = serde_json::from_str(FIRESTORM)?;
        assert_eq!(dump.viewer(), "firestorm");
        assert_eq!(dump.schema_version, Some(1));
        let object = dump.objects.first().ok_or("no objects")?;
        let face = object.faces.first().ok_or("no faces")?;
        assert_eq!(
            face.fullbright,
            Some(false),
            "an integer 0 is the same fact as a JSON false"
        );
        assert_eq!(object.is_flexible, None, "the reference omitted it");
        assert_eq!(dump.environment.day_position, None);
        let avatar = dump.avatars.first().ok_or("no avatars")?;
        assert_eq!(avatar.has_body, None);
        assert_eq!(avatar.is_fully_loaded, Some(true));
        Ok(())
    }

    /// This viewer's document reads, including the four keys only it writes.
    #[test]
    fn this_viewers_dialect_reads() -> Result<(), TestError> {
        let dump: SceneDump = serde_json::from_str(SL_CLIENT)?;
        assert_eq!(dump.viewer(), "sl-client");
        assert_eq!(dump.environment.day_position, Some(0.25));
        let avatar = dump.avatars.first().ok_or("no avatars")?;
        assert!(avatar.drawn_position.is_some());
        assert_eq!(avatar.has_body, Some(true));
        Ok(())
    }

    /// The reference's terrain patches, sky and water are scenery it built, not
    /// content the grid sent: `local_id` 0 and an `app-…` class. Reporting them
    /// as objects this viewer is missing would bury every real finding under
    /// 275 of them.
    #[test]
    fn viewer_side_scenery_is_recognised() -> Result<(), TestError> {
        let dump: SceneDump = serde_json::from_str(FIRESTORM)?;
        let scenery: Vec<&str> = dump
            .objects
            .iter()
            .filter(|object| object.is_viewer_scenery())
            .map(|object| object.id.as_str())
            .collect();
        assert_eq!(scenery, ["8f3a0000-0000-0000-0000-000000000001"]);
        let content = dump.objects.first().ok_or("no objects")?;
        assert!(!content.is_viewer_scenery(), "a rezzed prim is not scenery");
        Ok(())
    }

    /// A local id past `i32::MAX` arrives negative from the reference's `(S32)`
    /// cast and positive from this viewer: the same 32 bits, and the reader
    /// makes them one number rather than one difference.
    #[test]
    fn a_negative_local_id_is_the_same_thirty_two_bits() -> Result<(), TestError> {
        let dump: SceneDump =
            serde_json::from_str(r#"{ "objects": [ { "id": "a", "local_id": -2147483648 } ] }"#)?;
        let object = dump.objects.first().ok_or("no objects")?;
        assert_eq!(object.local_id(), Some(0x8000_0000));
        Ok(())
    }
}
