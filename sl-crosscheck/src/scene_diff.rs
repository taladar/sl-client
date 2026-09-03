//! The scene-dump diff: the output that **names the cause**.
//!
//! An image diff ranks two frames by how far apart they are. It cannot say
//! *why*, and the four commonest reasons look identical in a picture: a prim in
//! the wrong place, a texture that resolved to a different asset, a mesh stuck
//! at a coarser level of detail, a material that never arrived. This module
//! compares the two viewers' `scene.json` documents field by field, and its
//! findings are sentences like "face 2 of the mesh cube is texture A here and
//! texture B there".
//!
//! # What may be matched by id, and what may not
//!
//! A comparison keys objects by id, and **not every id in a scene is a grid
//! id**. Three kinds are minted locally — they differ between two viewers of one
//! scene, and between two runs of one viewer — so matching on them manufactures
//! findings:
//!
//! - **The reference's own scene objects.** Its terrain patches, sky, water and
//!   clouds are `LLViewerObject`s with `local_id` 0 and an `app-…` class: 275 of
//!   its 296 objects in the catalogue scene. This viewer does not model them as
//!   objects at all. They are counted and named as scenery
//!   ([`Object::is_viewer_scenery`](crate::dump::Object::is_viewer_scenery)),
//!   never reported as objects one viewer is missing.
//! - **Control avatars.** An animesh rides a headless avatar with no grid
//!   identity: the reference minted `36a77dc9…` and then `ecce83a2…` for the
//!   *same* animesh in two consecutive runs, while this viewer reports the
//!   animesh object's key. So they are paired by `is_control_avatar` and
//!   position, never by id.
//! - **Baked avatar textures**, whose ids are minted per bake by whoever baked
//!   them. Neither dump lists them today; the rule is recorded here because the
//!   day one does, matching them would read as a divergence on every avatar.
//!
//! Everything else here is a grid id: object keys, mesh assets, and the texture
//! ids of ordinary faces.
//!
//! # A difference is not automatically a divergence
//!
//! Some fields are known to mean different things on the two sides. They are
//! reported — hiding them would be worse — but carry a [`Finding::note`] and are
//! never ranked, because a reader who finds the top of the list occupied by
//! `num_faces` on every object learns to skip the list:
//!
//! - `num_faces` is the count this viewer **drew** and the count the reference
//!   **declares** (`getNumTEs`).
//! - `is_flexible` is "declares itself flexible" here against "is being drawn
//!   flexible" there.
//! - `aspect` reads 1.778 here against 1.388 there because Firestorm's snapshot
//!   renders at the capture's aspect while its `LLViewerCamera` reports the
//!   window's by the time the dump is written — an artefact of how the dump is
//!   taken, not of what was drawn.
//! - `has_body` here against `is_fully_loaded` there: two different questions
//!   about the same grey avatar.
//! - An animation's `loop_time` is where each viewer's clock had got to, and two
//!   frames of a 2 s loop half a second apart are two phases of one motion. This
//!   nearly cost a wrong bug report once already.
//! - The reference starts default motions on every avatar — head rotation, eye,
//!   body noise, breathing, physics, hand pose, pelvis fix — which this viewer
//!   implements as adjusters rather than as motions. They are named as such
//!   instead of ranked as animations we are missing.
//!
//! # The settings come first, because they are often the answer
//!
//! The first thing the first live pair of dumps found was not in the images at
//! all: the two `render` sections differed (`mesh_lod_boost` 1.0 against 2.0,
//! draw distance 512 m against 128 m), which *explains* the `lod` differences on
//! five objects. So [`SceneDiff::render`] prints the render, camera and
//! environment sections side by side **above** the findings. A report that ranks
//! differences without showing those hides its own answer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dump::{Animation, Avatar, Face, Object, Point, Quaternion, SceneDump};

/// The reference's default motions: the ones `LLVOAvatar` starts on every avatar
/// whether or not the simulator asked for them.
///
/// Baked from `indra/newview/llvoavatar.cpp` (the `ANIM_AGENT_*` globals) rather
/// than derived, because the constants are the ground truth and a generator that
/// re-derives them would be one more thing to keep working.
///
/// This viewer implements these as adjusters applied to a pose rather than as
/// motions in a playlist, so they appear on that side of a pair and not on this
/// one. That is a difference in how the two are built, not one in what they
/// drew, and it must be said in those words: a report listing seven "animations
/// sl-client is not playing" on every avatar teaches its reader to stop reading.
const REFERENCE_DEFAULT_MOTIONS: [(&str, &str); 11] = [
    ("9aa8b0a6-0c6f-9518-c7c3-4f41f2c001ad", "body_noise"),
    ("4c5a103e-b830-2f1c-16bc-224aa0ad5bc8", "breathe_rot"),
    ("2a8eba1d-a7f8-5596-d44a-b4977bf8c8bb", "editing"),
    ("5c780ea8-1cd1-c463-a128-48c023f6fbea", "eye"),
    ("db95561f-f1b0-9f9a-7224-b12f71af126e", "fly_adjust"),
    ("ce986325-0ba7-6e6e-cc24-b17c4b795578", "hand_motion"),
    ("e6e8d1dd-e643-fff7-b238-c6b4b056a68d", "head_rot"),
    ("0c5dd2a2-514d-8893-d44d-05beffad208b", "pelvis_fix"),
    ("0e4896cb-fba4-926c-f355-8720189d5b55", "target"),
    ("829bc85b-02fc-ec41-be2e-74cc6dd7215d", "walk_adjust"),
    ("7360e029-3cb8-ebc4-863e-212df440d987", "physics_motion"),
];

/// The name the reference gives one of its default motions, when the id is one.
fn default_motion_name(id: &str) -> Option<&'static str> {
    let id = id.to_ascii_lowercase();
    REFERENCE_DEFAULT_MOTIONS
        .into_iter()
        .find_map(|(known, name)| (known == id).then_some(name))
}

/// How far apart two numbers may be before it is worth saying so.
///
/// Not one epsilon: a millimetre matters on a position and is noise on a
/// texture repeat, and both viewers write `f32`, so the floor is what a `f32`
/// round-trip can move a number by rather than what a `f64` can.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// Metres, for positions and sizes.
    pub metres: f64,
    /// Degrees, for rotations.
    pub degrees: f64,
    /// The relative slack on every other number.
    pub relative: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            // A centimetre: below the resolution at which either viewer's own
            // transform maths is repeatable, and far under anything an eye sees
            // in a frame.
            metres: 0.01,
            // Half a degree: a quaternion written as four `f32` and read back
            // moves by far less, and nothing smaller is visible.
            degrees: 0.5,
            relative: 1e-3,
        }
    }
}

/// How much attention a finding deserves, and therefore where it sorts.
///
/// The order is the order in which the kinds of finding have actually explained
/// a frame: something wholly absent first, then an asset id that differs (which
/// *names* the divergence), then a number, then a flag. Annotations — the known
/// semantic differences — sort last always, whatever their magnitude, because
/// they are reported for completeness rather than for action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rank {
    /// Present on one side and not the other.
    Absent,
    /// An asset id that differs: a texture, a mesh, a material.
    Identity,
    /// A number that differs, ranked among its kind by how far.
    Numeric,
    /// A flag or a name that differs.
    Flag,
    /// A known semantic difference: reported, never ranked.
    Annotated,
}

/// One difference between the two dumps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// How much attention it deserves.
    pub rank: Rank,
    /// What it is about: an object, an avatar, the camera.
    pub subject: String,
    /// Which field.
    pub field: String,
    /// What this viewer's dump said.
    pub left: String,
    /// What the reference's dump said.
    pub right: String,
    /// How far apart, in the field's own units, when that is a number.
    pub distance: Option<f64>,
    /// Why this is not necessarily a divergence, when it is not.
    pub note: Option<String>,
}

impl Finding {
    /// The line a report prints for it.
    #[must_use]
    pub fn describe(&self) -> String {
        let distance = self
            .distance
            .map_or_else(String::new, |distance| format!(" (Δ {distance:.3})"));
        let note = self
            .note
            .as_deref()
            .map_or_else(String::new, |note| format!("\n      {note}"));
        format!(
            "  {}: {} — {} vs {}{}{}",
            self.subject, self.field, self.left, self.right, distance, note
        )
    }
}

/// One setting reported side by side whether or not it differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setting {
    /// The setting's name.
    pub name: String,
    /// This viewer's value.
    pub left: String,
    /// The reference's value.
    pub right: String,
    /// Whether the two agree.
    pub agrees: bool,
}

/// What one side of the comparison brought to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side {
    /// The viewer that wrote the dump.
    pub viewer: String,
    /// Its build.
    pub version: Option<String>,
    /// The region it was in.
    pub region: Option<String>,
    /// How many objects it reported, scenery excluded.
    pub objects: usize,
    /// How many of its objects were viewer-side scenery.
    pub scenery: usize,
    /// How many avatars it reported.
    pub avatars: usize,
}

impl Side {
    /// Summarise one dump.
    fn of(dump: &SceneDump) -> Self {
        let scenery = dump
            .objects
            .iter()
            .filter(|object| object.is_viewer_scenery())
            .count();
        Self {
            viewer: dump.viewer().to_owned(),
            version: dump.context.version.clone(),
            region: dump.context.region_name.clone(),
            objects: dump.objects.len().saturating_sub(scenery),
            scenery,
            avatars: dump.avatars.len(),
        }
    }
}

/// The comparison of two scene dumps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneDiff {
    /// The two schema versions, when they disagree — in which case nothing below
    /// was compared.
    pub schema_mismatch: Option<(Option<u32>, Option<u32>)>,
    /// This viewer's side.
    pub left: Side,
    /// The reference's side.
    pub right: Side,
    /// The camera, environment and render settings, side by side, always.
    pub settings: Vec<Setting>,
    /// Every difference found, worst first.
    pub findings: Vec<Finding>,
}

impl SceneDiff {
    /// Compare `left` (this viewer) against `right` (the reference).
    ///
    /// Two documents at different schema versions are not compared at all: the
    /// fields would be read under the wrong meaning, and a page of confident
    /// nonsense is worse than one line saying the pair cannot be diffed.
    #[must_use]
    pub fn compare(left: &SceneDump, right: &SceneDump, tolerances: Tolerances) -> Self {
        let mut diff = Self {
            schema_mismatch: None,
            left: Side::of(left),
            right: Side::of(right),
            settings: Vec::new(),
            findings: Vec::new(),
        };
        if left.schema_version != right.schema_version {
            diff.schema_mismatch = Some((left.schema_version, right.schema_version));
            return diff;
        }
        diff.settings = settings(left, right, tolerances);
        diff.findings = objects(left, right, tolerances);
        diff.findings.extend(avatars(left, right, tolerances));
        diff.findings.extend(camera(left, right, tolerances));
        diff.findings.extend(environment(left, right, tolerances));
        // Rank first, then magnitude within a rank, then subject so two runs of
        // the same pair print in the same order.
        diff.findings.sort_by(|first, second| {
            first.rank.cmp(&second.rank).then_with(|| {
                second
                    .distance
                    .unwrap_or(0.0)
                    .total_cmp(&first.distance.unwrap_or(0.0))
                    .then_with(|| first.subject.cmp(&second.subject))
                    .then_with(|| first.field.cmp(&second.field))
            })
        });
        diff
    }

    /// The findings that are actual divergences rather than annotated known
    /// differences.
    pub fn divergences(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|finding| finding.rank != Rank::Annotated)
    }

    /// The report's text.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "the only arithmetic is `index + 1` over a bounded list of findings"
    )]
    #[must_use]
    pub fn render(&self, limit: usize) -> String {
        let mut lines = Vec::new();
        if let Some((left, right)) = self.schema_mismatch {
            lines.push(format!(
                "the two dumps are written to different schema versions ({} and {}), so they were \
                 not compared: rebuild whichever viewer is behind",
                left.map_or_else(|| "absent".to_owned(), |version| version.to_string()),
                right.map_or_else(|| "absent".to_owned(), |version| version.to_string()),
            ));
            return lines.join("\n");
        }
        for side in [&self.left, &self.right] {
            lines.push(format!(
                "  {} {} — {} object(s){}, {} avatar(s){}",
                side.viewer,
                side.version.as_deref().unwrap_or("(no version)"),
                side.objects,
                if side.scenery == 0 {
                    String::new()
                } else {
                    format!(
                        " (plus {} viewer-side scene object(s): terrain, sky, water)",
                        side.scenery
                    )
                },
                side.avatars,
                side.region
                    .as_deref()
                    .map_or_else(String::new, |region| format!(" in {region}")),
            ));
        }
        // Above the findings, deliberately: a settings difference explains
        // findings below it, and a reader who meets the findings first goes
        // looking for a rendering bug that is a preset.
        lines.push(String::new());
        lines.push("settings (these decide what a frame could contain at all):".to_owned());
        for setting in &self.settings {
            lines.push(format!(
                "  {} {}: {} vs {}",
                if setting.agrees { " " } else { "!" },
                setting.name,
                setting.left,
                setting.right
            ));
        }

        let divergences: Vec<&Finding> = self.divergences().collect();
        lines.push(String::new());
        if divergences.is_empty() {
            lines.push("no divergence found in the scene dumps".to_owned());
        } else {
            lines.push(format!("{} divergence(s), worst first:", divergences.len()));
            for finding in divergences.iter().take(limit) {
                lines.push(finding.describe());
            }
            if divergences.len() > limit {
                // Never a silent cap: a list that stops without saying so reads
                // as a list that ended.
                lines.push(format!(
                    "  … and {} more (raise the limit to see them all)",
                    divergences.len() - limit
                ));
            }
        }

        let annotated: Vec<&Finding> = self
            .findings
            .iter()
            .filter(|finding| finding.rank == Rank::Annotated)
            .collect();
        if !annotated.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "{} known semantic difference(s) — reported, not ranked:",
                annotated.len()
            ));
            for finding in annotated.iter().take(limit) {
                lines.push(finding.describe());
            }
            if annotated.len() > limit {
                lines.push(format!("  … and {} more", annotated.len() - limit));
            }
        }
        lines.join("\n")
    }
}

/// The camera, environment and render settings, side by side.
fn settings(left: &SceneDump, right: &SceneDump, tolerances: Tolerances) -> Vec<Setting> {
    let mut settings = Vec::new();
    let mut number = |name: &str, first: Option<f64>, second: Option<f64>| {
        settings.push(Setting {
            name: name.to_owned(),
            left: show_number(first),
            right: show_number(second),
            agrees: agree(first, second, tolerances.relative),
        });
    };
    number(
        "draw_distance",
        left.render.draw_distance,
        right.render.draw_distance,
    );
    number(
        "mesh_lod_boost",
        left.render.mesh_lod_boost,
        right.render.mesh_lod_boost,
    );
    number(
        "quality_level",
        left.render.quality_level.map(int_to_float),
        right.render.quality_level.map(int_to_float),
    );
    number(
        "shadow_detail",
        left.render.shadow_detail.map(int_to_float),
        right.render.shadow_detail.map(int_to_float),
    );
    number(
        "max_texture_res",
        left.render.max_texture_res.map(int_to_float),
        right.render.max_texture_res.map(int_to_float),
    );
    number(
        "reflection_detail",
        left.render.reflection_detail.map(int_to_float),
        right.render.reflection_detail.map(int_to_float),
    );
    number(
        "fov_degrees",
        left.camera.fov_radians.map(f64::to_degrees),
        right.camera.fov_radians.map(f64::to_degrees),
    );
    number("near_clip", left.camera.near_clip, right.camera.near_clip);
    number("far_clip", left.camera.far_clip, right.camera.far_clip);
    settings.push(Setting {
        name: "sky_name".to_owned(),
        left: show_text(left.environment.sky_name.as_deref()),
        right: show_text(right.environment.sky_name.as_deref()),
        agrees: left.environment.sky_name == right.environment.sky_name,
    });
    settings.push(Setting {
        name: "water_name".to_owned(),
        left: show_text(left.environment.water_name.as_deref()),
        right: show_text(right.environment.water_name.as_deref()),
        agrees: left.environment.water_name == right.environment.water_name,
    });
    settings
}

/// Compare the two object lists.
fn objects(left: &SceneDump, right: &SceneDump, tolerances: Tolerances) -> Vec<Finding> {
    let ours = by_id(&left.objects);
    let theirs = by_id(&right.objects);
    let mut findings = Vec::new();
    for (id, object) in &ours {
        match theirs.get(id) {
            Some(other) => findings.extend(compare_object(object, other, tolerances)),
            None => findings.push(Finding {
                rank: Rank::Absent,
                subject: describe_object(object),
                field: "presence".to_owned(),
                left: "present".to_owned(),
                right: "absent".to_owned(),
                distance: None,
                note: None,
            }),
        }
    }
    for (id, object) in &theirs {
        if !ours.contains_key(id) {
            findings.push(Finding {
                rank: Rank::Absent,
                subject: describe_object(object),
                field: "presence".to_owned(),
                left: "absent".to_owned(),
                right: "present".to_owned(),
                distance: None,
                note: None,
            });
        }
    }
    findings
}

/// The objects that came from the grid, keyed by id — the viewer's own scenery
/// dropped, because neither viewer's copy of it exists in the other's.
fn by_id(objects: &[Object]) -> BTreeMap<String, &Object> {
    objects
        .iter()
        .filter(|object| !object.is_viewer_scenery())
        .map(|object| (object.id.to_ascii_lowercase(), object))
        .collect()
}

/// How a finding names an object.
fn describe_object(object: &Object) -> String {
    format!(
        "object {} ({})",
        short_id(&object.id),
        object.pcode.as_deref().unwrap_or("no pcode")
    )
}

/// Compare one object with its counterpart.
fn compare_object(ours: &Object, theirs: &Object, tolerances: Tolerances) -> Vec<Finding> {
    let subject = describe_object(ours);
    let mut findings = Vec::new();
    findings.extend(point_finding(
        &subject,
        "position",
        ours.position,
        theirs.position,
        tolerances.metres,
    ));
    findings.extend(rotation_finding(
        &subject,
        "rotation",
        ours.rotation,
        theirs.rotation,
        tolerances.degrees,
    ));
    findings.extend(point_finding(
        &subject,
        "scale",
        ours.scale,
        theirs.scale,
        tolerances.metres,
    ));
    findings.extend(text_finding(
        &subject,
        "pcode",
        ours.pcode.as_deref(),
        theirs.pcode.as_deref(),
        Rank::Flag,
    ));
    findings.extend(text_finding(
        &subject,
        "mesh_id",
        ours.mesh_id.as_deref(),
        theirs.mesh_id.as_deref(),
        Rank::Identity,
    ));
    for (field, ours, theirs) in [
        ("visible", ours.visible, theirs.visible),
        ("is_mesh", ours.is_mesh, theirs.is_mesh),
        ("is_sculpt", ours.is_sculpt, theirs.is_sculpt),
        ("is_light", ours.is_light, theirs.is_light),
    ] {
        findings.extend(flag_finding(&subject, field, ours, theirs));
    }
    findings.extend(number_finding(
        &subject,
        "lod",
        ours.lod.map(int_to_float),
        theirs.lod.map(int_to_float),
        tolerances.relative,
    ));
    // Two fields that mean different things on the two sides. Reported so a
    // reader can see them, annotated so they never crowd out a real finding.
    if let Some(mut finding) = number_finding(
        &subject,
        "num_faces",
        ours.num_faces.map(int_to_float),
        theirs.num_faces.map(int_to_float),
        tolerances.relative,
    ) {
        finding.rank = Rank::Annotated;
        finding.note = Some(
            "the count this viewer drew against the count the reference declares (getNumTEs)"
                .to_owned(),
        );
        findings.push(finding);
    }
    if let Some(mut finding) = flag_finding(
        &subject,
        "is_flexible",
        ours.is_flexible,
        theirs.is_flexible,
    ) {
        finding.rank = Rank::Annotated;
        finding.note = Some(
            "\"declares itself flexible\" here against \"is being drawn flexible\" there"
                .to_owned(),
        );
        findings.push(finding);
    }
    // A gap **inside our own dump**: an attachment composed onto its wearer the
    // reference's way, and drawn somewhere the wearer did not put it. The
    // reference emits no `drawn_position`, so this is a difference only one side
    // of the pair can see — which does not make it less of a bug.
    if let (Some(placed), Some(drawn)) = (ours.position, ours.drawn_position) {
        let distance = separation(placed, drawn);
        if distance > tolerances.metres {
            findings.push(Finding {
                rank: Rank::Numeric,
                subject: subject.clone(),
                field: "drawn_position".to_owned(),
                left: format!("{} drawn at {}", show_point(placed), show_point(drawn)),
                right: "(the reference emits no drawn_position)".to_owned(),
                distance: Some(distance),
                note: Some(
                    "this viewer drew a worn object away from where its wearer put it; both \
                     numbers are ours"
                        .to_owned(),
                ),
            });
        }
    }
    findings.extend(compare_faces(
        &subject,
        &ours.faces,
        &theirs.faces,
        tolerances,
    ));
    findings
}

/// Compare two face lists, by face index.
fn compare_faces(
    subject: &str,
    ours: &[Face],
    theirs: &[Face],
    tolerances: Tolerances,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let index = |faces: &[Face]| -> BTreeMap<i64, Face> {
        faces
            .iter()
            .enumerate()
            .map(|(position, face)| {
                (
                    face.index.unwrap_or_else(|| int_from_usize(position)),
                    face.clone(),
                )
            })
            .collect()
    };
    let ours = index(ours);
    let theirs = index(theirs);
    for (number, face) in &ours {
        let subject = format!("{subject} face {number}");
        let Some(other) = theirs.get(number) else {
            findings.push(Finding {
                rank: Rank::Absent,
                subject,
                field: "presence".to_owned(),
                left: "present".to_owned(),
                right: "absent".to_owned(),
                distance: None,
                note: None,
            });
            continue;
        };
        // The texture id first: it is the single field that most often turns "the
        // frames differ" into "this texture is not that texture".
        findings.extend(text_finding(
            &subject,
            "texture",
            face.texture.as_deref(),
            other.texture.as_deref(),
            Rank::Identity,
        ));
        findings.extend(text_finding(
            &subject,
            "material_id",
            face.material_id.as_deref(),
            other.material_id.as_deref(),
            Rank::Identity,
        ));
        for (field, ours, theirs) in [
            ("scale_s", face.scale_s, other.scale_s),
            ("scale_t", face.scale_t, other.scale_t),
            ("offset_s", face.offset_s, other.offset_s),
            ("offset_t", face.offset_t, other.offset_t),
            ("rotation", face.rotation, other.rotation),
            ("glow", face.glow, other.glow),
        ] {
            findings.extend(number_finding(
                &subject,
                field,
                ours,
                theirs,
                tolerances.relative,
            ));
        }
        for (field, ours, theirs) in [
            ("bump", face.bump, other.bump),
            ("shiny", face.shiny, other.shiny),
        ] {
            findings.extend(number_finding(
                &subject,
                field,
                ours.map(int_to_float),
                theirs.map(int_to_float),
                tolerances.relative,
            ));
        }
        findings.extend(flag_finding(
            &subject,
            "fullbright",
            face.fullbright,
            other.fullbright,
        ));
        if let (Some(ours), Some(theirs)) = (face.color, other.color) {
            let apart = ours
                .into_iter()
                .zip(theirs)
                .map(|(ours, theirs)| (ours - theirs).abs())
                .fold(0.0_f64, f64::max);
            if apart > tolerances.relative {
                findings.push(Finding {
                    rank: Rank::Numeric,
                    subject: subject.clone(),
                    field: "color".to_owned(),
                    left: show_colour(ours),
                    right: show_colour(theirs),
                    distance: Some(apart),
                    note: None,
                });
            }
        }
    }
    for number in theirs.keys() {
        if !ours.contains_key(number) {
            findings.push(Finding {
                rank: Rank::Absent,
                subject: format!("{subject} face {number}"),
                field: "presence".to_owned(),
                left: "absent".to_owned(),
                right: "present".to_owned(),
                distance: None,
                note: None,
            });
        }
    }
    findings
}

/// Compare the two avatar lists.
///
/// Residents match by agent id. Control avatars cannot: the reference mints a
/// local UUID for one and this viewer reports the animesh object's key, so they
/// are paired by position — the one thing about a headless avatar two viewers
/// can agree on.
fn avatars(left: &SceneDump, right: &SceneDump, tolerances: Tolerances) -> Vec<Finding> {
    let is_control = |avatar: &&Avatar| avatar.is_control_avatar == Some(true);
    let mut findings = Vec::new();

    let ours: BTreeMap<String, &Avatar> = left
        .avatars
        .iter()
        .filter(|avatar| !is_control(avatar))
        .map(|avatar| (avatar.id.to_ascii_lowercase(), avatar))
        .collect();
    let theirs: BTreeMap<String, &Avatar> = right
        .avatars
        .iter()
        .filter(|avatar| !is_control(avatar))
        .map(|avatar| (avatar.id.to_ascii_lowercase(), avatar))
        .collect();
    for (id, avatar) in &ours {
        match theirs.get(id) {
            Some(other) => findings.extend(compare_avatar(
                &format!("avatar {}", short_id(&avatar.id)),
                avatar,
                other,
                tolerances,
            )),
            None => findings.push(absence(&format!("avatar {}", short_id(&avatar.id)), true)),
        }
    }
    for (id, avatar) in &theirs {
        if !ours.contains_key(id) {
            findings.push(absence(&format!("avatar {}", short_id(&avatar.id)), false));
        }
    }

    // Control avatars: paired by proximity, nearest first, each used once.
    let mut unmatched: Vec<&Avatar> = right.avatars.iter().filter(is_control).collect();
    for avatar in left.avatars.iter().filter(is_control) {
        let nearest = avatar.position.and_then(|position| {
            unmatched
                .iter()
                .enumerate()
                .filter_map(|(index, other)| {
                    other
                        .position
                        .map(|theirs| (index, separation(position, theirs)))
                })
                .min_by(|(_first, first), (_second, second)| first.total_cmp(second))
        });
        let subject = format!("control avatar on object {}", short_id(&avatar.id));
        match nearest {
            // A metre: an animesh's control avatar sits at its object, and two
            // viewers placing it a metre apart is the finding, not a reason to
            // call them different animeshes.
            Some((index, distance)) if distance <= 1.0 => {
                let other = unmatched.swap_remove(index);
                findings.extend(compare_avatar(&subject, avatar, other, tolerances));
            }
            _too_far_or_absent => findings.push(absence(&subject, true)),
        }
    }
    for avatar in unmatched {
        findings.push(absence(
            &format!(
                "control avatar the reference minted as {}",
                short_id(&avatar.id)
            ),
            false,
        ));
    }
    findings
}

/// A "present on one side only" finding.
fn absence(subject: &str, ours: bool) -> Finding {
    Finding {
        rank: Rank::Absent,
        subject: subject.to_owned(),
        field: "presence".to_owned(),
        left: if ours { "present" } else { "absent" }.to_owned(),
        right: if ours { "absent" } else { "present" }.to_owned(),
        distance: None,
        note: None,
    }
}

/// Compare one avatar with its counterpart.
fn compare_avatar(
    subject: &str,
    ours: &Avatar,
    theirs: &Avatar,
    tolerances: Tolerances,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(point_finding(
        subject,
        "position",
        ours.position,
        theirs.position,
        tolerances.metres,
    ));
    findings.extend(rotation_finding(
        subject,
        "rotation",
        ours.rotation,
        theirs.rotation,
        tolerances.degrees,
    ));
    // Two different questions about the same grey avatar, so this is put side by
    // side rather than diffed.
    if let (Some(body), Some(loaded)) = (ours.has_body, theirs.is_fully_loaded)
        && body != loaded
    {
        findings.push(Finding {
            rank: Rank::Annotated,
            subject: subject.to_owned(),
            field: "has_body / is_fully_loaded".to_owned(),
            left: body.to_string(),
            right: loaded.to_string(),
            distance: None,
            note: Some(
                "\"the rigged base body has been built\" here against the reference's \
                 \"everything about this avatar has arrived\""
                    .to_owned(),
            ),
        });
    }
    findings.extend(compare_animations(
        subject,
        &ours.animations,
        &theirs.animations,
    ));
    findings
}

/// Compare what two viewers say one avatar is playing.
fn compare_animations(subject: &str, ours: &[Animation], theirs: &[Animation]) -> Vec<Finding> {
    let index = |animations: &[Animation]| -> BTreeMap<String, Animation> {
        animations
            .iter()
            .map(|animation| (animation.id.to_ascii_lowercase(), animation.clone()))
            .collect()
    };
    let ours = index(ours);
    let theirs = index(theirs);
    let mut findings = Vec::new();
    for (id, animation) in &ours {
        let subject = format!("{subject} animation {}", short_id(id));
        let Some(other) = theirs.get(id) else {
            findings.push(absence(&subject, true));
            continue;
        };
        // `time` is deliberately not compared: it counts from whenever each
        // viewer started playing, and the two started at different moments.
        if let (Some(ours), Some(theirs)) = (animation.loop_time, other.loop_time) {
            findings.push(Finding {
                rank: Rank::Annotated,
                subject: subject.clone(),
                field: "loop_time".to_owned(),
                left: format!("{ours:.2} s"),
                right: format!("{theirs:.2} s"),
                distance: Some((ours - theirs).abs()),
                note: Some(
                    "where each viewer's clock had reached in the motion: two frames of one \
                     loop, not two viewers disagreeing — a contact sheet of a moving avatar \
                     compares two phases"
                        .to_owned(),
                ),
            });
        }
        findings.extend(flag_finding(
            &subject,
            "looping",
            animation.looping,
            other.looping,
        ));
        findings.extend(number_finding(
            &subject,
            "priority",
            animation.priority.map(int_to_float),
            other.priority.map(int_to_float),
            1e-6,
        ));
        findings.extend(number_finding(
            &subject,
            "duration",
            animation.duration,
            other.duration,
            1e-2,
        ));
    }
    for id in theirs.keys() {
        if ours.contains_key(id) {
            continue;
        }
        let subject = format!("{subject} animation {}", short_id(id));
        match default_motion_name(id) {
            Some(name) => findings.push(Finding {
                rank: Rank::Annotated,
                subject,
                field: "presence".to_owned(),
                left: "not a motion here".to_owned(),
                right: format!("playing {name}"),
                distance: None,
                note: Some(
                    "one of the reference's default motions, which this viewer implements as a \
                     pose adjuster rather than as an animation"
                        .to_owned(),
                ),
            }),
            None => findings.push(absence(&subject, false)),
        }
    }
    findings
}

/// Compare the two cameras.
fn camera(left: &SceneDump, right: &SceneDump, tolerances: Tolerances) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (field, ours, theirs) in [
        (
            "origin_region",
            left.camera.origin_region,
            right.camera.origin_region,
        ),
        (
            "focus_region",
            left.camera.focus_region,
            right.camera.focus_region,
        ),
    ] {
        findings.extend(point_finding(
            "camera",
            field,
            ours,
            theirs,
            tolerances.metres,
        ));
    }
    for (field, ours, theirs) in [
        ("at_axis", left.camera.at_axis, right.camera.at_axis),
        ("up_axis", left.camera.up_axis, right.camera.up_axis),
        ("left_axis", left.camera.left_axis, right.camera.left_axis),
    ] {
        // Unit vectors: a centimetre of tolerance on a metre-long axis is the
        // wrong unit, so these get the relative one.
        findings.extend(point_finding(
            "camera",
            field,
            ours,
            theirs,
            tolerances.relative,
        ));
    }
    findings.extend(number_finding(
        "camera",
        "fov_degrees",
        left.camera.fov_radians.map(f64::to_degrees),
        right.camera.fov_radians.map(f64::to_degrees),
        tolerances.relative,
    ));
    // An artefact of how the reference takes its dump, not of what it drew: its
    // snapshot renders at the capture's aspect while `LLViewerCamera` reports
    // the window's by the time the dump is written.
    if let Some(mut finding) = number_finding(
        "camera",
        "aspect",
        left.camera.aspect,
        right.camera.aspect,
        tolerances.relative,
    ) {
        finding.rank = Rank::Annotated;
        finding.note = Some(
            "the reference's snapshot renders at the capture's aspect while its LLViewerCamera \
             reports the window's by the time the dump is written"
                .to_owned(),
        );
        findings.push(finding);
    }
    findings
}

/// Compare the two skies.
fn environment(left: &SceneDump, right: &SceneDump, tolerances: Tolerances) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (field, ours, theirs) in [
        (
            "sun_direction",
            left.environment.sun_direction,
            right.environment.sun_direction,
        ),
        (
            "moon_direction",
            left.environment.moon_direction,
            right.environment.moon_direction,
        ),
    ] {
        findings.extend(point_finding(
            "environment",
            field,
            ours,
            theirs,
            tolerances.relative,
        ));
    }
    findings.extend(rotation_finding(
        "environment",
        "sun_rotation",
        left.environment.sun_rotation,
        right.environment.sun_rotation,
        tolerances.degrees,
    ));
    for (field, ours, theirs) in [
        (
            "sky_name",
            left.environment.sky_name.as_deref(),
            right.environment.sky_name.as_deref(),
        ),
        (
            "water_name",
            left.environment.water_name.as_deref(),
            right.environment.water_name.as_deref(),
        ),
    ] {
        findings.extend(text_finding("environment", field, ours, theirs, Rank::Flag));
    }
    findings
}

/// A finding for two positions, when they are far enough apart to matter.
fn point_finding(
    subject: &str,
    field: &str,
    ours: Option<Point>,
    theirs: Option<Point>,
    tolerance: f64,
) -> Option<Finding> {
    let (ours, theirs) = both(ours, theirs)?;
    let distance = separation(ours, theirs);
    (distance > tolerance).then(|| Finding {
        rank: Rank::Numeric,
        subject: subject.to_owned(),
        field: field.to_owned(),
        left: show_point(ours),
        right: show_point(theirs),
        distance: Some(distance),
        note: None,
    })
}

/// A finding for two orientations, measured as the angle between them.
fn rotation_finding(
    subject: &str,
    field: &str,
    ours: Option<Quaternion>,
    theirs: Option<Quaternion>,
    tolerance_degrees: f64,
) -> Option<Finding> {
    let (ours, theirs) = both(ours, theirs)?;
    let degrees = angle_between(ours, theirs);
    (degrees > tolerance_degrees).then(|| Finding {
        rank: Rank::Numeric,
        subject: subject.to_owned(),
        field: field.to_owned(),
        left: show_quaternion(ours),
        right: show_quaternion(theirs),
        distance: Some(degrees),
        note: None,
    })
}

/// A finding for two numbers, compared with a relative tolerance.
fn number_finding(
    subject: &str,
    field: &str,
    ours: Option<f64>,
    theirs: Option<f64>,
    relative: f64,
) -> Option<Finding> {
    let (ours, theirs) = both(ours, theirs)?;
    (!agree(Some(ours), Some(theirs), relative)).then(|| Finding {
        rank: Rank::Numeric,
        subject: subject.to_owned(),
        field: field.to_owned(),
        left: format!("{ours:.4}"),
        right: format!("{theirs:.4}"),
        distance: Some((ours - theirs).abs()),
        note: None,
    })
}

/// A finding for two flags.
fn flag_finding(
    subject: &str,
    field: &str,
    ours: Option<bool>,
    theirs: Option<bool>,
) -> Option<Finding> {
    let (ours, theirs) = both(ours, theirs)?;
    (ours != theirs).then(|| Finding {
        rank: Rank::Flag,
        subject: subject.to_owned(),
        field: field.to_owned(),
        left: ours.to_string(),
        right: theirs.to_string(),
        distance: None,
        note: None,
    })
}

/// A finding for two strings — an asset id, a name, a class.
fn text_finding(
    subject: &str,
    field: &str,
    ours: Option<&str>,
    theirs: Option<&str>,
    rank: Rank,
) -> Option<Finding> {
    let (ours, theirs) = both(ours, theirs)?;
    (!ours.eq_ignore_ascii_case(theirs)).then(|| Finding {
        rank,
        subject: subject.to_owned(),
        field: field.to_owned(),
        left: ours.to_owned(),
        right: theirs.to_owned(),
        distance: None,
        note: None,
    })
}

/// Both values, when both sides reported one.
///
/// A field only one side writes is **absent**, not different: `day_position`,
/// `drawn_position` and `has_body` are this viewer's, `visual_complexity` and
/// `camera_mode` are the reference's, and a comparison that treated a missing
/// key as a value would report every one of them on every object.
fn both<T>(ours: Option<T>, theirs: Option<T>) -> Option<(T, T)> {
    Some((ours?, theirs?))
}

/// Whether two numbers agree within a relative tolerance.
fn agree(ours: Option<f64>, theirs: Option<f64>, relative: f64) -> bool {
    match (ours, theirs) {
        (Some(ours), Some(theirs)) => {
            let scale = ours.abs().max(theirs.abs()).max(1.0);
            (ours - theirs).abs() <= relative * scale
        }
        (None, None) => true,
        _one_side_only => false,
    }
}

/// The distance between two points, in metres.
fn separation(ours: Point, theirs: Point) -> f64 {
    let [x, y, z] = ours;
    let [other_x, other_y, other_z] = theirs;
    ((x - other_x).powi(2) + (y - other_y).powi(2) + (z - other_z).powi(2)).sqrt()
}

/// The angle between two orientations, in degrees.
///
/// Through the absolute dot product, because `q` and `-q` are the same rotation
/// and the two viewers do not agree on which of them to write.
fn angle_between(ours: Quaternion, theirs: Quaternion) -> f64 {
    let norm = |quaternion: Quaternion| {
        let [x, y, z, w] = quaternion;
        (x * x + y * y + z * z + w * w).sqrt()
    };
    let (ours_length, theirs_length) = (norm(ours), norm(theirs));
    if ours_length <= f64::EPSILON || theirs_length <= f64::EPSILON {
        return 0.0;
    }
    let [x, y, z, w] = ours;
    let [other_x, other_y, other_z, other_w] = theirs;
    let dot =
        (x * other_x + y * other_y + z * other_z + w * other_w) / (ours_length * theirs_length);
    (2.0 * dot.abs().clamp(0.0, 1.0).acos()).to_degrees()
}

/// A `f64` of an `i64`, for comparing two whole numbers with the same machinery
/// as every other number.
fn int_to_float(value: i64) -> f64 {
    // `i64 as f64` is lossy past 2^53; every integer in this schema is a level
    // of detail, a face count or a settings enum, so the conversion is exact and
    // the fallback is only there so it cannot silently be otherwise.
    i32::try_from(value).map_or(f64::NAN, f64::from)
}

/// The face index of a face that did not write one: its position in the list.
fn int_from_usize(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The first eight characters of an id, which is what a person reads.
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// A point, as a report prints one.
fn show_point(point: Point) -> String {
    let [x, y, z] = point;
    format!("<{x:.3}, {y:.3}, {z:.3}>")
}

/// A quaternion, as a report prints one.
fn show_quaternion(quaternion: Quaternion) -> String {
    let [x, y, z, w] = quaternion;
    format!("<{x:.3}, {y:.3}, {z:.3}, {w:.3}>")
}

/// A colour, as a report prints one.
fn show_colour(colour: [f64; 4]) -> String {
    let [red, green, blue, alpha] = colour;
    format!("<{red:.3}, {green:.3}, {blue:.3}, {alpha:.3}>")
}

/// A number, or that the viewer did not report one.
fn show_number(value: Option<f64>) -> String {
    value.map_or_else(|| "(absent)".to_owned(), |value| format!("{value:.4}"))
}

/// A string, or that the viewer did not report one.
fn show_text(value: Option<&str>) -> String {
    value.map_or_else(|| "(absent)".to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Rank, SceneDiff, Tolerances};
    use crate::dump::SceneDump;

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// Read a dump from JSON written inline.
    fn dump(json: &str) -> Result<SceneDump, TestError> {
        Ok(serde_json::from_str(json)?)
    }

    /// A pair that differs only in the reference's own scenery: 256 terrain
    /// patches and a sky it models as objects and this viewer does not. Not one
    /// finding, and the count is said out loud instead.
    #[test]
    fn the_references_own_scenery_is_not_a_finding() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1, "context": { "viewer": "sl-client" },
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume",
                                "position": [1.0, 2.0, 3.0] } ] }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1, "context": { "viewer": "firestorm" },
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume",
                                "position": [1.0, 2.0, 3.0] },
                              { "id": "bbbb0000-0000-0000-0000-000000000002",
                                "local_id": 0, "pcode": "app-30" },
                              { "id": "cccc0000-0000-0000-0000-000000000003",
                                "local_id": 0, "pcode": "app-31" } ] }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        assert_eq!(diff.divergences().count(), 0);
        assert_eq!(diff.right.scenery, 2);
        assert_eq!(diff.right.objects, 1);
        assert!(diff.render(20).contains("viewer-side scene object"));
        Ok(())
    }

    /// The finding the whole tool exists for: one face, two texture ids. It
    /// ranks above a numeric difference, because it names the cause where a
    /// number only measures it.
    #[test]
    fn a_texture_that_differs_outranks_a_position_that_differs() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1,
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume",
                                "position": [1.0, 2.0, 3.0],
                                "faces": [ { "index": 0,
                                             "texture": "11111111-0000-0000-0000-000000000000" } ] },
                              { "id": "dddd0000-0000-0000-0000-000000000004",
                                "local_id": 8, "pcode": "volume",
                                "position": [10.0, 0.0, 0.0] } ] }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1,
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume",
                                "position": [1.0, 2.0, 3.0],
                                "faces": [ { "index": 0,
                                             "texture": "22222222-0000-0000-0000-000000000000" } ] },
                              { "id": "dddd0000-0000-0000-0000-000000000004",
                                "local_id": 8, "pcode": "volume",
                                "position": [10.0, 0.0, 2.5] } ] }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        let first = diff.divergences().next().ok_or("no divergence")?;
        assert_eq!(first.rank, Rank::Identity);
        assert_eq!(first.field, "texture");
        let second = diff.divergences().nth(1).ok_or("only one divergence")?;
        assert_eq!(second.field, "position");
        assert_eq!(second.distance.map(|distance| distance.round()), Some(3.0));
        Ok(())
    }

    /// The default motions the reference starts on every avatar are named for
    /// what they are, not ranked as animations this viewer failed to play.
    #[test]
    fn the_references_default_motions_are_annotated_not_ranked() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1,
                 "avatars": [ { "id": "5b1f0000-0000-0000-0000-000000000002",
                                "position": [1.0, 1.0, 1.0], "animations": [] } ] }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1,
                 "avatars": [ { "id": "5b1f0000-0000-0000-0000-000000000002",
                                "position": [1.0, 1.0, 1.0],
                                "animations": [
                                  { "id": "e6e8d1dd-e643-fff7-b238-c6b4b056a68d" },
                                  { "id": "7360e029-3cb8-ebc4-863e-212df440d987" },
                                  { "id": "abcd0000-0000-0000-0000-000000000009" } ] } ] }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        let divergences: Vec<&super::Finding> = diff.divergences().collect();
        assert_eq!(
            divergences.len(),
            1,
            "only the animation the simulator named is a divergence"
        );
        let only = divergences.first().ok_or("no divergence")?;
        assert!(only.subject.contains("abcd0000"));
        assert_eq!(
            diff.findings
                .iter()
                .filter(|finding| finding.rank == Rank::Annotated)
                .count(),
            2,
            "head_rot and physics_motion are adjusters here, not missing motions"
        );
        Ok(())
    }

    /// A control avatar's id is minted by whoever is looking at it, so the pair
    /// is made by position. Matching on the id would report both viewers as
    /// missing an animesh that both of them drew.
    #[test]
    fn control_avatars_pair_by_position_not_by_id() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1,
                 "avatars": [ { "id": "0b1ec700-0000-0000-0000-00000000000f",
                                "is_control_avatar": true,
                                "position": [10.0, 10.0, 20.0], "animations": [] } ] }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1,
                 "avatars": [ { "id": "36a77dc9-0000-0000-0000-000000000000",
                                "is_control_avatar": true,
                                "position": [10.0, 10.0, 20.0], "animations": [] } ] }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        assert_eq!(
            diff.divergences().count(),
            0,
            "one animesh drawn by both viewers is not two missing ones"
        );
        Ok(())
    }

    /// The settings are reported whether or not they differ, and above the
    /// findings: `mesh_lod_boost` 1.0 against 2.0 is what explains a `lod`
    /// difference, and a report that buries it hides its own answer.
    #[test]
    fn the_render_settings_are_reported_and_explain_the_findings() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1,
                 "render": { "draw_distance": 512.0, "mesh_lod_boost": 1.0 },
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume", "lod": 2 } ] }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1,
                 "render": { "draw_distance": 128.0, "mesh_lod_boost": 2.0 },
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume", "lod": 3 } ] }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        let boost = diff
            .settings
            .iter()
            .find(|setting| setting.name == "mesh_lod_boost")
            .ok_or("no mesh_lod_boost")?;
        assert!(!boost.agrees);
        let report = diff.render(20);
        let settings_at = report.find("settings").ok_or("no settings section")?;
        let findings_at = report.find("divergence(s)").ok_or("no findings section")?;
        assert!(
            settings_at < findings_at,
            "the settings that explain a finding must be printed above it"
        );
        Ok(())
    }

    /// Two dumps at different schema versions are not compared at all: reading
    /// one document under the other's meaning produces confident nonsense.
    #[test]
    fn a_schema_mismatch_stops_the_comparison() -> Result<(), TestError> {
        let ours = dump(r#"{ "schema_version": 1 }"#)?;
        let theirs = dump(r#"{ "schema_version": 2 }"#)?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        assert!(diff.schema_mismatch.is_some());
        assert!(diff.findings.is_empty());
        assert!(diff.render(20).contains("different schema versions"));
        Ok(())
    }

    /// A field only one viewer writes is absent, not different. This viewer's
    /// `day_position` and the reference's `visual_complexity` would otherwise
    /// each be a finding on every run.
    #[test]
    fn a_field_only_one_side_writes_is_not_a_finding() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1,
                 "environment": { "day_position": 0.25, "sky_name": "Default" } }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1,
                 "environment": { "sky_name": "Default", "selected": 0 } }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        assert_eq!(diff.findings.len(), 0);
        Ok(())
    }

    /// A worn object drawn away from where its wearer put it is a difference
    /// inside our own dump — the reference emits no `drawn_position`, which does
    /// not make it less of a bug.
    #[test]
    fn an_attachment_drawn_off_its_wearer_is_reported() -> Result<(), TestError> {
        let ours = dump(
            r#"{ "schema_version": 1,
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume",
                                "position": [10.0, 10.0, 26.2],
                                "drawn_position": [10.0, 10.0, 27.06] } ] }"#,
        )?;
        let theirs = dump(
            r#"{ "schema_version": 1,
                 "objects": [ { "id": "aaaa0000-0000-0000-0000-000000000001",
                                "local_id": 7, "pcode": "volume",
                                "position": [10.0, 10.0, 26.2] } ] }"#,
        )?;
        let diff = SceneDiff::compare(&ours, &theirs, Tolerances::default());
        let finding = diff
            .findings
            .iter()
            .find(|finding| finding.field == "drawn_position")
            .ok_or("the drawn/placed gap was not reported")?;
        assert_eq!(
            finding.distance.map(|distance| (distance * 100.0).round()),
            Some(86.0)
        );
        Ok(())
    }
}
