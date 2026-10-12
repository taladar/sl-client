//! A census of materials on a prim: what each grid keeps per face, who may
//! change it, and how everybody is told.
//!
//! A face has up to three things beyond its texture, kept two ways:
//!
//! - a **legacy material** — a normal and a specular map with their
//!   parameters. A viewer PUTs it to the `RenderMaterials` capability; the
//!   region gives it an id, writes the id into the face's texture entry and
//!   answers a POST of the id with the material.
//! - a **PBR material** — a GLTF material asset — and an **override** of some
//!   of its fields. A viewer POSTs both to `ModifyMaterialParams`; the region
//!   names the asset in the object's extra parameters and sends the overrides
//!   in a `GenericStreamingMessage` of their own.
//!
//! The case rezzes a cube as the primary avatar, with a second avatar
//! standing by, and asks one question a leg. After each it notes what the
//! capability answered, what changed on the cube as the owner's stream states
//! it, what each avatar was sent, and whether the other avatar's stream
//! states what the owner's does:
//!
//! 1. **The capabilities**, and what `RenderMaterials` answers a read it
//!    cannot serve.
//! 2. **A legacy material**: a set, the id it gets, the read back, the same
//!    set again, the same material on another face, another material, the
//!    viewer's default material, two faces at once, a face the prim has not
//!    got, a prim the region has not got, the other avatar's set, a clear,
//!    and a texture entry that states or leaves out a material id.
//! 3. **A PBR material**: an override with no material under it, the
//!    material, an override, the same override, another, both at once, text
//!    that is no GLTF, an asset that does not exist, the other avatar's, a
//!    legacy material on the same face, the override cleared, the material
//!    set again and cleared, and every face at once.
//! 4. **The client's own commands.**
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/materials.md`; every run is held to it.
//!
//! `2av`, live grids only: the fake grid acknowledges a material edit and
//! drops it (`server-fake-grid-material-edits`). The cube is derezzed to the
//! Trash, also when a leg fails.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, FaceMaterialPut, LegacyMaterial, MaterialOverrideUpdate, Object, ObjectKey,
    ObjectUpdateForm, RegionLocalObjectId, RenderMaterialEntry, ScopedObjectId, TextureKey, Uuid,
    decode_texture_entry,
};
use sl_material::{
    GltfAlphaMode, MaterialOverride, TextureOverride, TextureTransformOverride,
    encode_override_gltf_json, parse_material_override,
};
use sl_wire::{
    Llsd, build_modify_material_params_request, build_render_materials_put_request,
    build_render_materials_request, parse_llsd_xml, parse_render_materials_envelope,
    parse_render_materials_response,
};

use crate::cases::object_media::{answered, rendered};
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, clean_up, rez_cube,
};
use crate::cases::task_inventory::{
    CapAnswer, DRAIN, FAKE_DRAIN, NO_CAPABILITY, OPENSIM_START, find_object, hold_to_measured,
    http_client, login_folders, shape_of,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene};

/// How many faces the cube has.
const CUBE_FACES: usize = 6;

/// The reference viewer's blank GLTF material (`BLANK_MATERIAL_ASSET_ID`,
/// `indra_constants.cpp`), which every Second Life region knows.
const BLANK_MATERIAL: Uuid = Uuid::from_u128(0x968c_bad0_4dad_d64e_71b5_72bf_13ad_051a);

/// An asset id no grid has an asset under.
const NO_SUCH_ASSET: Uuid = Uuid::from_u128(0x0000_0bad_a55e_7000_8000_0000_0000_dead);

/// A material id no region made.
const NO_SUCH_MATERIAL: Uuid = Uuid::from_u128(0x0000_0bad_3a7e_7000_8000_0000_0000_dead);

/// A local id no region has a prim under.
const NO_SUCH_PRIM: u32 = 0x7fff_fff0;

/// The texture the materials name as their normal and base-colour map (the
/// viewer's plywood, which every grid has).
const A_TEXTURE: Uuid = Uuid::from_u128(0x8955_6747_24cb_43ed_920b_47ca_ed15_465f);

/// The texture the materials name as their specular map (the viewer's blank
/// white).
const ANOTHER_TEXTURE: Uuid = Uuid::from_u128(0x5748_decc_f629_461c_9a36_a35a_221f_e21f);

/// How far two reals a grid sent back may be from the ones it was sent.
const REAL_SLACK: f32 = 1e-3;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/materials.md (materials, 2026-10-12)";

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under. Every run is held to all of it at the end, so a run that disagrees
/// says everything it disagrees about at once.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "client_legacy_fetch_states",
        both(Answer::Text("every field")),
    ),
    (
        "client_legacy_set_changes",
        both(Answer::Text("face 5: legacy=A")),
    ),
    (
        "client_legacy_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "client_legacy_set_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "client_legacy_set_told_owner",
        both(Answer::Text("full update")),
    ),
    (
        "client_pbr_set_changes",
        each(
            Answer::Text("face 4: pbr=the blank material"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "client_pbr_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "client_pbr_set_result",
        each(Answer::Text("success=true"), Answer::Text("no event")),
    ),
    (
        "client_pbr_set_told_other",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "client_pbr_set_told_owner",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "fresh_cube_states",
        both(Answer::Text("legacy none; pbr none; overrides on none")),
    ),
    (
        "legacy_all_answers",
        each(
            Answer::Text(
                "200: zipped, entries of {ID:binary,Material:{AlphaMaskCutoff:integer,DiffuseAlphaMode:integer,EnvIntensity:integer,NormMap:uuid,NormOffsetX:integer,NormOffsetY:integer,NormRepeatX:integer,NormRepeatY:integer,NormRotation:integer,SpecColor:[4],SpecExp:integer,SpecMap:uuid,SpecOffsetX:integer,SpecOffsetY:integer,SpecRepeatX:integer,SpecRepeatY:integer,SpecRotation:integer}}",
            ),
            Answer::Text("200: zipped, entries"),
        ),
    ),
    (
        "legacy_all_lists_it",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "legacy_another_material_set_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    (
        "legacy_another_material_set_changes",
        both(Answer::Text("face 0: legacy=B")),
    ),
    (
        "legacy_another_material_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_another_material_set_told_other",
        each(Answer::Text("terse update"), Answer::Text("full update")),
    ),
    (
        "legacy_another_material_set_told_owner",
        each(Answer::Text("terse update"), Answer::Text("full update")),
    ),
    (
        "legacy_clear_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    (
        "legacy_clear_changes",
        both(Answer::Text("face 0: legacy=none")),
    ),
    (
        "legacy_clear_of_a_bare_face_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    (
        "legacy_clear_of_a_bare_face_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_clear_of_a_bare_face_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_clear_of_a_bare_face_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_clear_of_a_bare_face_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_clear_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    ("legacy_clear_told_other", both(Answer::Text("full update"))),
    ("legacy_clear_told_owner", both(Answer::Text("full update"))),
    (
        "legacy_default_material_id",
        each(
            Answer::Text("20ddc75e-772a-57b1-1585-3df4c69e3388"),
            Answer::Text("none"),
        ),
    ),
    (
        "legacy_default_material_set_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    (
        "legacy_default_material_set_changes",
        each(Answer::Text("face 2: legacy=C"), Answer::Text("nothing")),
    ),
    (
        "legacy_default_material_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_default_material_set_told_other",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "legacy_default_material_set_told_owner",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "legacy_empty_post_answers",
        each(Answer::Text("400: no body"), Answer::Text("200: no body")),
    ),
    (
        "legacy_empty_put_answers",
        each(Answer::Text("400: no body"), Answer::Text("200: no body")),
    ),
    (
        "legacy_image_leaves_the_id_out_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_leaves_the_id_out_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_image_leaves_the_id_out_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_leaves_the_id_out_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_states_a_known_id_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_states_a_known_id_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_image_states_a_known_id_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_states_a_known_id_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_states_an_unknown_id_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_states_an_unknown_id_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_image_states_an_unknown_id_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_image_states_an_unknown_id_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_material_id",
        each(
            Answer::Text("daa9877a-44db-df92-d24d-fedd34d65f3a"),
            Answer::Text("2516b40d-5fa0-e456-5321-ac92185cf727"),
        ),
    ),
    (
        "legacy_no_face_set_answers",
        each(
            Answer::Text("400: no body"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    ("legacy_no_face_set_changes", both(Answer::Text("nothing"))),
    (
        "legacy_no_face_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_no_face_set_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_no_face_set_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_no_id_answers",
        both(Answer::Text("200: zipped, entries")),
    ),
    (
        "legacy_no_such_face_set_answers",
        both(Answer::Text("200: zipped, 0 entries")),
    ),
    (
        "legacy_no_such_face_set_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_no_such_face_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_no_such_face_set_told_other",
        each(Answer::Text("nothing"), Answer::Text("full update")),
    ),
    (
        "legacy_no_such_face_set_told_owner",
        each(Answer::Text("nothing"), Answer::Text("full update")),
    ),
    (
        "legacy_nonsense_post_answers",
        each(Answer::Text("400: no body"), Answer::Text("200: no body")),
    ),
    (
        "legacy_nonsense_put_answers",
        each(Answer::Text("400: no body"), Answer::Text("200: no body")),
    ),
    (
        "legacy_other_clear_answers",
        both(Answer::Text("200: zipped, 0 entries")),
    ),
    ("legacy_other_clear_changes", both(Answer::Text("nothing"))),
    (
        "legacy_other_clear_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_other_clear_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_other_clear_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_other_read_states",
        both(Answer::Text("what the owner's does")),
    ),
    (
        "legacy_other_set_answers",
        both(Answer::Text("200: zipped, 0 entries")),
    ),
    ("legacy_other_set_changes", both(Answer::Text("nothing"))),
    (
        "legacy_other_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    ("legacy_other_set_told_other", both(Answer::Text("nothing"))),
    ("legacy_other_set_told_owner", both(Answer::Text("nothing"))),
    (
        "legacy_read_answers",
        both(Answer::Text(
            "200: zipped, 1 entries of {ID:binary,Material:{AlphaMaskCutoff:integer,DiffuseAlphaMode:integer,EnvIntensity:integer,NormMap:uuid,NormOffsetX:integer,NormOffsetY:integer,NormRepeatX:integer,NormRepeatY:integer,NormRotation:integer,SpecColor:[4],SpecExp:integer,SpecMap:uuid,SpecOffsetX:integer,SpecOffsetY:integer,SpecRepeatX:integer,SpecRepeatY:integer,SpecRotation:integer}}",
        )),
    ),
    (
        "legacy_read_twice_answers",
        both(Answer::Text(
            "200: zipped, 2 entries of {ID:binary,Material:{AlphaMaskCutoff:integer,DiffuseAlphaMode:integer,EnvIntensity:integer,NormMap:uuid,NormOffsetX:integer,NormOffsetY:integer,NormRepeatX:integer,NormRepeatY:integer,NormRotation:integer,SpecColor:[4],SpecExp:integer,SpecMap:uuid,SpecOffsetX:integer,SpecOffsetY:integer,SpecRepeatX:integer,SpecRepeatY:integer,SpecRotation:integer}}",
        )),
    ),
    (
        "legacy_same_set_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    ("legacy_same_set_changes", both(Answer::Text("nothing"))),
    (
        "legacy_same_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    ("legacy_same_set_told_other", both(Answer::Text("nothing"))),
    ("legacy_same_set_told_owner", both(Answer::Text("nothing"))),
    (
        "legacy_second_face_set_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    (
        "legacy_second_face_set_changes",
        both(Answer::Text("face 1: legacy=A")),
    ),
    (
        "legacy_second_face_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_second_face_set_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "legacy_second_face_set_told_owner",
        both(Answer::Text("full update")),
    ),
    (
        "legacy_set_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    ("legacy_set_changes", both(Answer::Text("face 0: legacy=A"))),
    (
        "legacy_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    ("legacy_set_round_trips", both(Answer::Text("every field"))),
    ("legacy_set_told_other", both(Answer::Text("full update"))),
    ("legacy_set_told_owner", both(Answer::Text("full update"))),
    (
        "legacy_states",
        each(
            Answer::Text("legacy 1=A,2=C,3=A,4=B; pbr none; overrides on none"),
            Answer::Text("legacy 1=A,3=A,4=B; pbr none; overrides on none"),
        ),
    ),
    (
        "legacy_two_faces_set_answers",
        each(
            Answer::Text("200: zipped, 2 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text("200: zipped, 0 entries"),
        ),
    ),
    (
        "legacy_two_faces_set_changes",
        both(Answer::Text("face 3: legacy=A; face 4: legacy=B")),
    ),
    (
        "legacy_two_faces_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_two_faces_set_told_other",
        both(Answer::Text("full update")),
    ),
    (
        "legacy_two_faces_set_told_owner",
        both(Answer::Text("full update")),
    ),
    (
        "legacy_unknown_id_answers",
        both(Answer::Text("200: zipped, 0 entries")),
    ),
    (
        "legacy_unknown_prim_set_answers",
        both(Answer::Text("200: zipped, 0 entries")),
    ),
    (
        "legacy_unknown_prim_set_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_unknown_prim_set_other_sees",
        both(Answer::Text("what the owner does")),
    ),
    (
        "legacy_unknown_prim_set_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "legacy_unknown_prim_set_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "material_agent_inventory_cap_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "material_task_inventory_cap_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "modify_cap_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "pbr_all_faces_asset_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_asset_changes",
        each(
            Answer::Text(
                "face 0: pbr=the blank material; face 2: pbr=the blank material; face 3: pbr=the blank material; face 4: pbr=the blank material; face 5: pbr=the blank material",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_asset_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_asset_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_all_faces_asset_told_owner",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_all_faces_cleared_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_cleared_changes",
        each(
            Answer::Text(
                "face 0: pbr=none; face 1: pbr=none; face 2: pbr=none; face 3: pbr=none; face 4: pbr=none; face 5: pbr=none",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_cleared_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_cleared_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_all_faces_cleared_told_owner",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_all_faces_override_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_override_changes",
        each(
            Answer::Text(
                "face 1: override={'bc':[r1,r0,r0,r1],'rf':r0.5}; face 2: override={'bc':[r1,r0,r0,r1],'rf':r0.5}; face 3: override={'bc':[r1,r0,r0,r1],'rf':r0.5}; face 4: override={'bc':[r1,r0,r0,r1],'rf':r0.5}; face 5: override={'bc':[r1,r0,r0,r1],'rf':r0.5}",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_override_told_other",
        each(
            Answer::Text("1 override message(s), the last naming 0,1,2,3,4,5"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_all_faces_override_told_owner",
        each(
            Answer::Text("1 override message(s), the last naming 0,1,2,3,4,5"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_asset_under_an_override_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_asset_under_an_override_changes",
        each(
            Answer::Text("face 0: pbr=the asset that does not exist"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_asset_under_an_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_asset_under_an_override_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_another_asset_under_an_override_told_owner",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_another_override_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_override_changes",
        each(
            Answer::Text("face 0: override={'mf':r0.25}"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_override_told_other",
        each(
            Answer::Text("1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_another_override_told_owner",
        each(
            Answer::Text("1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asked_again_after_the_clear_told_other",
        each(
            Answer::Text("full update+1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asked_again_at_the_end_told_other",
        each(
            Answer::Text("full update+1 override message(s), the last naming 0,1,2,3,4,5"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asked_again_told_other",
        each(
            Answer::Text("full update+1 override message(s), the last naming 1"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_changes",
        each(
            Answer::Text("face 0: pbr=the blank material"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_cleared_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_cleared_changes",
        each(
            Answer::Text("face 0: pbr=none"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_cleared_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_cleared_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_asset_cleared_told_owner",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_asset_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_asset_told_owner",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_asset_with_override_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_with_override_changes",
        each(
            Answer::Text(
                "face 1: pbr=the blank material,override={'ac':r0.25,'am':i2,'bc':[r0.5,r0.25,r1,r0.75],'ds':1,'ec':[r0.5,r0.25,r0],'mf':r0.75,'rf':r0.25,'tex':[u89556747-24cb-43ed-920b-47caed15465f],'ti':[{'o':[r0.25,r0.5],'r':r0.5,'s':[r2,r3]}]}",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_with_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_with_override_round_trips",
        each(Answer::Text("every field"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_asset_with_override_told_other",
        each(
            Answer::Text("full update+1 override message(s), the last naming 0,1"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_asset_with_override_told_owner",
        each(
            Answer::Text("full update+1 override message(s), the last naming 0,1"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_bare_update_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_bare_update_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_bare_update_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_bare_update_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_bare_update_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_empty_body_answers",
        each(
            Answer::Text(
                "400: success=false, message=missing or invalid required parameter object_id",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_http_get_answers",
        each(Answer::Text("405: no body"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_legacy_on_a_pbr_face_answers",
        each(
            Answer::Text("200: zipped, 1 entries of {Face:integer,ID:integer,MaterialID:binary}"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_legacy_on_a_pbr_face_changes",
        each(
            Answer::Text("face 1: legacy=B"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_legacy_on_a_pbr_face_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_legacy_on_a_pbr_face_told_other",
        each(Answer::Text("terse update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_legacy_on_a_pbr_face_told_owner",
        each(Answer::Text("terse update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_map_body_answers",
        each(
            Answer::Text(
                "400: success=false, message=missing or invalid required parameter object_id",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_no_such_face_answers",
        each(
            Answer::Text("404: success=false, message=Invalid side: 9"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_no_such_face_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_no_such_face_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_no_such_face_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_no_such_face_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_no_update_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_nonsense_override_answers",
        each(
            Answer::Text(
                "400: success=false, message=Warnings/errors parsing GLTF JSON:  / [json.exception.parse_error.101] parse error at line 1, column 2: syntax error while parsing value - invalid literal; last read: 'no'",
            ),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_nonsense_override_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_nonsense_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_nonsense_override_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_nonsense_override_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_other_asset_answers",
        each(
            Answer::Text("403: success=false, message=permission denied"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_other_asset_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_other_asset_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_other_asset_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_other_asset_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_other_override_answers",
        each(
            Answer::Text("403: success=false, message=permission denied"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_other_override_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_other_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_other_override_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_other_override_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_override_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_override_cleared_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_cleared_changes",
        each(
            Answer::Text("face 1: override=none"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_cleared_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_cleared_told_other",
        each(
            Answer::Text("1 override message(s), the last naming no face"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_cleared_told_owner",
        each(
            Answer::Text("1 override message(s), the last naming no face"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_of_nothing_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_of_nothing_changes",
        each(
            Answer::Text("face 0: override=none"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_of_nothing_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_of_nothing_told_other",
        each(
            Answer::Text("1 override message(s), the last naming 1"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_of_nothing_told_owner",
        each(
            Answer::Text("1 override message(s), the last naming 1"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_round_trips",
        each(Answer::Text("every field"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_override_set_again_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_set_again_changes",
        each(
            Answer::Text("face 0: override={'bc':[r1,r0,r0,r1],'rf':r0.5}"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_set_again_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_set_again_told_other",
        each(
            Answer::Text("1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_set_again_told_owner",
        each(
            Answer::Text("1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_override_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_override_without_a_material_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_without_a_material_changes",
        each(
            Answer::Text("face 0: override={'bc':[r1,r0,r0,r1],'rf':r0.5}"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_without_a_material_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_without_a_material_told_other",
        each(
            Answer::Text("1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_override_without_a_material_told_owner",
        each(
            Answer::Text("1 override message(s), the last naming 0"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_same_asset_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_same_asset_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_same_asset_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_same_asset_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_same_asset_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_same_override_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_same_override_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_same_override_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_same_override_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_same_override_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_states",
        each(
            Answer::Text("legacy 1=B,2=C,3=A,4=B; pbr none; overrides on 0,1,2,3,4,5"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_unknown_asset_answers",
        each(
            Answer::Text("200: success=true"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_unknown_asset_changes",
        each(
            Answer::Text("face 2: pbr=the asset that does not exist"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_unknown_asset_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_unknown_asset_told_other",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_unknown_asset_told_owner",
        each(Answer::Text("full update"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_unknown_prim_answers",
        each(
            Answer::Text("404: success=false, message=failed to find task"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_unknown_prim_changes",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_unknown_prim_other_sees",
        each(
            Answer::Text("what the owner does"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "pbr_unknown_prim_told_other",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "pbr_unknown_prim_told_owner",
        each(Answer::Text("nothing"), Answer::Text(NO_CAPABILITY)),
    ),
    ("render_materials_cap_granted", both(Answer::Flag(true))),
];

/// Which avatar asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The cube's owner.
    Owner,
    /// The avatar standing by.
    Other,
}

/// How a capability is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    /// An HTTP `GET`.
    Get,
    /// An HTTP `POST`.
    Post,
    /// An HTTP `PUT`.
    Put,
}

/// One avatar's capabilities.
#[derive(Debug, Clone)]
struct Caps {
    /// `RenderMaterials`.
    render: String,
    /// `ModifyMaterialParams`, where the region grants it.
    modify: Option<String>,
}

impl Caps {
    /// The capabilities `session` was granted, if it was granted
    /// `RenderMaterials`.
    fn of(session: &Session) -> Option<Self> {
        Some(Self {
            render: session.cap("RenderMaterials")?,
            modify: session.cap("ModifyMaterialParams"),
        })
    }
}

/// What a capability answered, as it was written.
#[derive(Debug)]
struct Raw {
    /// The HTTP status.
    status: u16,
    /// The body.
    text: String,
}

impl Raw {
    /// The answer with its body read as LLSD.
    fn answer(&self) -> CapAnswer {
        CapAnswer {
            status: self.status,
            body: parse_llsd_xml(&self.text).ok(),
        }
    }

    /// What the answer's `Zipped` member unzips to, where it has one.
    fn unzipped(&self) -> Option<Llsd> {
        parse_render_materials_envelope(&self.text).ok().flatten()
    }

    /// The materials the answer lists, where it is a listing.
    fn materials(&self) -> Vec<RenderMaterialEntry> {
        parse_render_materials_response(&self.text).unwrap_or_default()
    }

    /// A `RenderMaterials` answer as a metric records it: the status, and
    /// what was zipped into it — how many entries (where `counted`) and the
    /// shape of the first.
    fn zipped(&self, counted: bool) -> String {
        match self.unzipped() {
            Some(Llsd::Array(items)) => {
                let of = items
                    .first()
                    .map_or_else(String::new, |item| format!(" of {}", shape_of(item)));
                if counted {
                    format!("{}: zipped, {} entries{of}", self.status, items.len())
                } else {
                    format!("{}: zipped, entries{of}", self.status)
                }
            }
            Some(other) => format!("{}: zipped {}", self.status, shape_of(&other)),
            None => answered(&self.answer()),
        }
    }

    /// A `ModifyMaterialParams` answer as a metric records it: the status,
    /// whether it reports success, and its message.
    fn modified(&self) -> String {
        let answer = self.answer();
        let Some(Llsd::Boolean(success)) =
            answer.body.as_ref().and_then(|body| body.get("success"))
        else {
            return answered(&answer);
        };
        let message = answer
            .body
            .as_ref()
            .and_then(|body| body.get("message"))
            .map(rendered)
            .unwrap_or_default();
        if message.is_empty() {
            format!("{}: success={success}", self.status)
        } else {
            format!("{}: success={success}, message={message}", self.status)
        }
    }
}

/// Asks the capability at `url`.
async fn ask(
    http: &reqwest::Client,
    verb: Verb,
    url: &str,
    body: String,
) -> Result<Raw, TestFailure> {
    let request = match verb {
        Verb::Get => http.get(url).header("Accept", "application/llsd+xml"),
        Verb::Post => http
            .post(url)
            .header("Content-Type", "application/llsd+xml")
            .body(body),
        Verb::Put => http
            .put(url)
            .header("Content-Type", "application/llsd+xml")
            .body(body),
    };
    let response = request
        .send()
        .await
        .map_err(|error| TestFailure::Assertion(format!("a capability request failed: {error}")))?;
    let status = response.status().as_u16();
    Ok(Raw {
        status,
        text: response.text().await.unwrap_or_default(),
    })
}

/// The names the census gives the ids a grid mints, so that a metric can say
/// "the same material" without stating an id.
#[derive(Debug, Default)]
struct Names {
    /// Every minted id, in the order it was first seen.
    seen: Vec<Uuid>,
}

impl Names {
    /// The name of `id`: what it is where the census chose it, a letter in
    /// order of first sight where a grid did.
    fn of(&mut self, id: Uuid) -> String {
        if id == BLANK_MATERIAL {
            return "the blank material".to_owned();
        }
        if id == NO_SUCH_ASSET {
            return "the asset that does not exist".to_owned();
        }
        if id == NO_SUCH_MATERIAL {
            return "the id no region made".to_owned();
        }
        let index = self
            .seen
            .iter()
            .position(|seen| *seen == id)
            .unwrap_or_else(|| {
                self.seen.push(id);
                self.seen.len().saturating_sub(1)
            });
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .nth(index)
            .map_or_else(|| format!("#{index}"), String::from)
    }

    /// The name of what a face may have: `none`, or the id's.
    fn of_face(&mut self, id: Option<Uuid>) -> String {
        id.map_or_else(|| "none".to_owned(), |id| self.of(id))
    }
}

/// The cube's materials as one avatar's stream states them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct State {
    /// The legacy material id each face's texture entry states.
    legacy: Vec<Option<Uuid>>,
    /// The GLTF material asset the extra parameters name for each face.
    pbr: Vec<Option<Uuid>>,
    /// The override the last override message stated for each face that has
    /// one, as it was written.
    overrides: BTreeMap<u8, String>,
}

impl State {
    /// The state of a cube nothing has been sent about.
    fn bare() -> Self {
        Self {
            legacy: vec![None; CUBE_FACES],
            pbr: vec![None; CUBE_FACES],
            overrides: BTreeMap::new(),
        }
    }

    /// Takes the legacy ids and the GLTF assets from `object`.
    fn note_object(&mut self, object: &Object) {
        self.legacy = decode_texture_entry(&object.texture_entry, CUBE_FACES)
            .faces
            .iter()
            .map(|face| face.material_id.filter(|id| !id.is_nil()))
            .collect();
        self.pbr = (0..CUBE_FACES)
            .map(|face| {
                object
                    .extra
                    .render_material
                    .iter()
                    .find(|named| usize::from(named.face) == face)
                    .map(|named| named.material_id)
            })
            .collect();
    }

    /// Takes what `told` stated: the cube as it was last sent, and the
    /// overrides of the last override message, which names every face that
    /// has one.
    fn note(&mut self, told: &Told) {
        if let Some(object) = &told.latest {
            self.note_object(object);
        }
        if let Some(message) = told.overrides.last() {
            self.overrides = message
                .faces
                .iter()
                .copied()
                .zip(message.documents.iter().cloned())
                .collect();
        }
    }

    /// The state as one metric.
    fn summary(&self, names: &mut Names) -> String {
        let faces = |ids: &[Option<Uuid>], names: &mut Names| {
            let stated: Vec<String> = ids
                .iter()
                .enumerate()
                .filter_map(|(face, id)| id.map(|id| format!("{face}={}", names.of(id))))
                .collect();
            if stated.is_empty() {
                "none".to_owned()
            } else {
                stated.join(",")
            }
        };
        let overridden: Vec<String> = self.overrides.keys().map(u8::to_string).collect();
        format!(
            "legacy {}; pbr {}; overrides on {}",
            faces(&self.legacy, names),
            faces(&self.pbr, names),
            if overridden.is_empty() {
                "none".to_owned()
            } else {
                overridden.join(",")
            }
        )
    }
}

/// What `after` states that `before` does not, face by face — or `nothing`.
fn changes(before: &State, after: &State, names: &mut Names) -> String {
    let mut parts = Vec::new();
    for face in 0..CUBE_FACES {
        let mut fields = Vec::new();
        let legacy = |state: &State| state.legacy.get(face).copied().flatten();
        if legacy(before) != legacy(after) {
            fields.push(format!("legacy={}", names.of_face(legacy(after))));
        }
        let pbr = |state: &State| state.pbr.get(face).copied().flatten();
        if pbr(before) != pbr(after) {
            fields.push(format!("pbr={}", names.of_face(pbr(after))));
        }
        let key = u8::try_from(face).unwrap_or(u8::MAX);
        if before.overrides.get(&key) != after.overrides.get(&key) {
            fields.push(format!(
                "override={}",
                after.overrides.get(&key).map_or("none", String::as_str)
            ));
        }
        if !fields.is_empty() {
            parts.push(format!("face {face}: {}", fields.join(",")));
        }
    }
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("; ")
    }
}

/// One override message about the cube.
#[derive(Debug, Clone)]
struct OverrideMessage {
    /// How long after the request was sent it was read.
    after: Duration,
    /// The faces it named.
    faces: Vec<u8>,
    /// The override of each, as written.
    documents: Vec<String>,
}

/// What one avatar was sent about the cube during a window.
#[derive(Debug, Default)]
struct Told {
    /// The forms the cube was sent again in.
    forms: Vec<ObjectUpdateForm>,
    /// The cube as it read after the last update that changed it.
    latest: Option<Object>,
    /// Every override message about the cube.
    overrides: Vec<OverrideMessage>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// Every `ModifyMaterialParams` result the client reported.
    results: Vec<(bool, String)>,
    /// Every `RenderMaterials` listing the client reported.
    fetched: Vec<Vec<RenderMaterialEntry>>,
}

impl Told {
    /// The window as one metric: the forms the cube came in, how many
    /// override messages and the faces the last named, and any alert — or
    /// `nothing`.
    fn summary(&self) -> String {
        let mut parts: Vec<String> = [
            (ObjectUpdateForm::Full, "full update"),
            (ObjectUpdateForm::Compressed, "compressed update"),
            (ObjectUpdateForm::Terse, "terse update"),
        ]
        .into_iter()
        .filter(|(form, _word)| self.forms.contains(form))
        .map(|(_form, word)| word.to_owned())
        .collect();
        if let Some(last) = self.overrides.last() {
            let faces: Vec<String> = last.faces.iter().map(u8::to_string).collect();
            parts.push(format!(
                "{} override message(s), the last naming {}",
                self.overrides.len(),
                if faces.is_empty() {
                    "no face".to_owned()
                } else {
                    faces.join(",")
                }
            ));
        }
        if !self.alerts.is_empty() {
            parts.push(format!("alert: {}", self.alerts.join(" | ")));
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join("+")
        }
    }
}

/// Listens on `session` for `window` and notes what it is sent about the
/// cube `cube`, known in the region as `local`. `sent` is when the request
/// the window follows was sent.
async fn hear(
    session: &mut Session,
    cube: ObjectKey,
    local: RegionLocalObjectId,
    window: Duration,
    sent: Instant,
) -> Result<Told, TestFailure> {
    let mut told = Told::default();
    let outcome = session
        .wait_for(window, |event| {
            match event {
                Event::ObjectStreamBatch(batch) if !batch.child => {
                    let named = batch
                        .entries
                        .iter()
                        .any(|entry| entry.known && entry.local_id == local);
                    if named && !told.forms.contains(&batch.form) {
                        told.forms.push(batch.form);
                    }
                }
                Event::ObjectUpdated(object) if object.full_id == cube => {
                    told.latest = Some((**object).clone());
                }
                Event::GltfMaterialOverride {
                    local_id,
                    faces,
                    overrides,
                    ..
                } if local_id.id == local => told.overrides.push(OverrideMessage {
                    after: sent.elapsed(),
                    faces: faces.clone(),
                    documents: overrides
                        .iter()
                        .map(|document| String::from_utf8_lossy(document).into_owned())
                        .collect(),
                }),
                Event::AlertMessage {
                    message,
                    alert_info,
                    ..
                } => told.alerts.push(alert_info.first().map_or_else(
                    || message.clone(),
                    |info| format!("{message} [{}]", info.message),
                )),
                Event::AgentAlertMessage { message, .. } => told.alerts.push(message.clone()),
                Event::MaterialParamsResult { success, message } => {
                    told.results.push((*success, message.clone()));
                }
                Event::RenderMaterials(entries) => told.fetched.push(entries.clone()),
                _ => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(told),
        Err(other) => Err(other),
    }
}

/// What one request brought.
struct Posted {
    /// What the capability answered, where the request went to one.
    answer: Option<String>,
    /// The owner's view before.
    before: State,
    /// The owner's view after.
    after: State,
    /// The other avatar's view after.
    others: State,
    /// What the owner was sent.
    to_owner: Told,
    /// What the other avatar was sent.
    to_other: Told,
}

/// A legacy material with something else in every field.
fn shiny() -> LegacyMaterial {
    LegacyMaterial {
        normal_map: TextureKey::from(A_TEXTURE),
        normal_offset: (0.25, -0.25),
        normal_repeat: (2.0, 3.0),
        normal_rotation: 0.5,
        specular_map: TextureKey::from(ANOTHER_TEXTURE),
        specular_offset: (0.125, 0.5),
        specular_repeat: (4.0, 0.5),
        specular_rotation: -0.25,
        specular_color: [10, 20, 30, 255],
        specular_exponent: 77,
        environment_intensity: 33,
        diffuse_alpha_mode: 2,
        alpha_mask_cutoff: 128,
    }
}

/// The reference viewer's default material (`LLMaterial::LLMaterial`): no
/// maps, white highlights, alpha blending.
fn plain() -> LegacyMaterial {
    LegacyMaterial {
        normal_map: TextureKey::from(Uuid::nil()),
        normal_offset: (0.0, 0.0),
        normal_repeat: (1.0, 1.0),
        normal_rotation: 0.0,
        specular_map: TextureKey::from(Uuid::nil()),
        specular_offset: (0.0, 0.0),
        specular_repeat: (1.0, 1.0),
        specular_rotation: 0.0,
        specular_color: [255; 4],
        specular_exponent: 51,
        environment_intensity: 0,
        diffuse_alpha_mode: 1,
        alpha_mask_cutoff: 0,
    }
}

/// Another legacy material: the default with an alpha mask.
fn matte() -> LegacyMaterial {
    LegacyMaterial {
        specular_exponent: 10,
        diffuse_alpha_mode: 2,
        alpha_mask_cutoff: 64,
        ..plain()
    }
}

/// Whether two reals are the same to [`REAL_SLACK`].
fn close(left: f32, right: f32) -> bool {
    (left - right).abs() <= REAL_SLACK
}

/// The fields `sent` and `got` differ in.
fn material_differences(sent: &LegacyMaterial, got: &LegacyMaterial) -> Vec<&'static str> {
    let pair =
        |left: (f32, f32), right: (f32, f32)| close(left.0, right.0) && close(left.1, right.1);
    [
        ("NormMap", sent.normal_map == got.normal_map),
        ("NormOffset", pair(sent.normal_offset, got.normal_offset)),
        ("NormRepeat", pair(sent.normal_repeat, got.normal_repeat)),
        (
            "NormRotation",
            close(sent.normal_rotation, got.normal_rotation),
        ),
        ("SpecMap", sent.specular_map == got.specular_map),
        (
            "SpecOffset",
            pair(sent.specular_offset, got.specular_offset),
        ),
        (
            "SpecRepeat",
            pair(sent.specular_repeat, got.specular_repeat),
        ),
        (
            "SpecRotation",
            close(sent.specular_rotation, got.specular_rotation),
        ),
        ("SpecColor", sent.specular_color == got.specular_color),
        ("SpecExp", sent.specular_exponent == got.specular_exponent),
        (
            "EnvIntensity",
            sent.environment_intensity == got.environment_intensity,
        ),
        (
            "DiffuseAlphaMode",
            sent.diffuse_alpha_mode == got.diffuse_alpha_mode,
        ),
        (
            "AlphaMaskCutoff",
            sent.alpha_mask_cutoff == got.alpha_mask_cutoff,
        ),
    ]
    .into_iter()
    .filter(|(_name, same)| !same)
    .map(|(name, _same)| name)
    .collect()
}

/// `every field`, or the fields that differ.
fn round_trip(differences: &[&str]) -> String {
    if differences.is_empty() {
        "every field".to_owned()
    } else {
        format!("but for {}", differences.join(","))
    }
}

/// An override of the base colour and the roughness.
fn tint() -> MaterialOverride {
    MaterialOverride {
        base_color: Some([1.0, 0.0, 0.0, 1.0]),
        roughness_factor: Some(0.5),
        ..MaterialOverride::default()
    }
}

/// An override of the metallic factor only.
fn metal() -> MaterialOverride {
    MaterialOverride {
        metallic_factor: Some(0.25),
        ..MaterialOverride::default()
    }
}

/// An override of every kind of field: a texture and its placement, the
/// factors, the alpha mode and the two-sided flag.
fn rich() -> MaterialOverride {
    let mut over = MaterialOverride {
        base_color: Some([0.5, 0.25, 1.0, 0.75]),
        emissive_factor: Some([0.5, 0.25, 0.0]),
        metallic_factor: Some(0.75),
        roughness_factor: Some(0.25),
        alpha_mode: Some(GltfAlphaMode::Mask),
        alpha_cutoff: Some(0.25),
        double_sided: Some(true),
        ..MaterialOverride::default()
    };
    if let Some(slot) = over.textures.first_mut() {
        *slot = Some(TextureOverride::Set(TextureKey::from(A_TEXTURE)));
    }
    if let Some(slot) = over.transforms.first_mut() {
        *slot = TextureTransformOverride {
            offset: Some([0.25, 0.5]),
            scale: Some([2.0, 3.0]),
            rotation: Some(0.5),
        };
    }
    over
}

/// The fields the overrides `sent` and `got` differ in.
fn override_differences(sent: &MaterialOverride, got: &MaterialOverride) -> Vec<&'static str> {
    let reals = |left: Option<&[f32]>, right: Option<&[f32]>| match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| close(*left, *right))
        }
        _ => false,
    };
    let real = |left: Option<f32>, right: Option<f32>| match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => close(left, right),
        _ => false,
    };
    let transforms = sent
        .transforms
        .iter()
        .zip(got.transforms.iter())
        .all(|(sent, got)| {
            reals(
                sent.offset.as_ref().map(<[f32; 2]>::as_slice),
                got.offset.as_ref().map(<[f32; 2]>::as_slice),
            ) && reals(
                sent.scale.as_ref().map(<[f32; 2]>::as_slice),
                got.scale.as_ref().map(<[f32; 2]>::as_slice),
            ) && real(sent.rotation, got.rotation)
        });
    [
        ("textures", sent.textures == got.textures),
        ("transforms", transforms),
        (
            "base_color",
            reals(
                sent.base_color.as_ref().map(<[f32; 4]>::as_slice),
                got.base_color.as_ref().map(<[f32; 4]>::as_slice),
            ),
        ),
        (
            "emissive_factor",
            reals(
                sent.emissive_factor.as_ref().map(<[f32; 3]>::as_slice),
                got.emissive_factor.as_ref().map(<[f32; 3]>::as_slice),
            ),
        ),
        (
            "metallic_factor",
            real(sent.metallic_factor, got.metallic_factor),
        ),
        (
            "roughness_factor",
            real(sent.roughness_factor, got.roughness_factor),
        ),
        ("alpha_mode", sent.alpha_mode == got.alpha_mode),
        ("alpha_cutoff", real(sent.alpha_cutoff, got.alpha_cutoff)),
        ("double_sided", sent.double_sided == got.double_sided),
    ]
    .into_iter()
    .filter(|(_name, same)| !same)
    .map(|(name, _same)| name)
    .collect()
}

/// A count as a metric.
fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// Everything the legs share.
struct Census<'a> {
    /// The HTTP client the capabilities are asked with.
    http: reqwest::Client,
    /// The avatar that owns the cube.
    primary: &'a mut Session,
    /// The avatar standing by.
    secondary: &'a mut Session,
    /// How long to listen.
    pace: Pace,
    /// How long the other avatar's queue is read for.
    drain: Duration,
    /// The cube, as the owner was last sent it.
    cube: Object,
    /// The cube as the other avatar knows it.
    others_cube: ScopedObjectId,
    /// The owner's capabilities.
    owners: Caps,
    /// The other avatar's.
    others: Caps,
    /// The cube's materials as the owner's stream states them.
    state: State,
    /// The cube's materials as the other avatar's stream states them.
    others_state: State,
    /// The names of the ids the grid minted.
    names: Names,
}

impl Census<'_> {
    /// The capabilities of `by`.
    const fn caps(&self, by: By) -> &Caps {
        match by {
            By::Owner => &self.owners,
            By::Other => &self.others,
        }
    }

    /// The cube's local id as the capabilities take it.
    const fn local(&self) -> u32 {
        self.cube.local_id.0
    }

    /// A `RenderMaterials` request of `by`, and what it left behind.
    async fn render(&mut self, by: By, verb: Verb, body: String) -> Result<Posted, TestFailure> {
        let sent = Instant::now();
        let raw = ask(&self.http, verb, &self.caps(by).render, body).await?;
        self.after(Some(raw.zipped(true)), sent).await
    }

    /// A PUT of `updates` to `RenderMaterials` by `by`.
    async fn put(&mut self, by: By, updates: &[FaceMaterialPut]) -> Result<Posted, TestFailure> {
        self.render(by, Verb::Put, build_render_materials_put_request(updates))
            .await
    }

    /// A PUT of `material` on `face` of the cube by `by`.
    async fn put_face(
        &mut self,
        by: By,
        face: u8,
        material: Option<LegacyMaterial>,
    ) -> Result<Posted, TestFailure> {
        let update = FaceMaterialPut {
            local_id: self.local(),
            face,
            material,
        };
        self.put(by, &[update]).await
    }

    /// The materials `RenderMaterials` lists for `ids` when `by` asks.
    async fn fetch(&self, by: By, ids: &[Uuid]) -> Result<Raw, TestFailure> {
        ask(
            &self.http,
            Verb::Post,
            &self.caps(by).render,
            build_render_materials_request(ids),
        )
        .await
    }

    /// POSTs `body` to the `ModifyMaterialParams` of `by`, where the region
    /// grants one.
    async fn modify_raw(&mut self, by: By, body: String) -> Result<Option<Posted>, TestFailure> {
        let Some(url) = self.caps(by).modify.clone() else {
            return Ok(None);
        };
        let sent = Instant::now();
        let raw = ask(&self.http, Verb::Post, &url, body).await?;
        self.after(Some(raw.modified()), sent).await.map(Some)
    }

    /// A `ModifyMaterialParams` update of `side` of the cube by `by`.
    async fn modify(
        &mut self,
        by: By,
        side: i32,
        asset_id: Option<Uuid>,
        gltf_json: Option<String>,
    ) -> Result<Option<Posted>, TestFailure> {
        let update = MaterialOverrideUpdate {
            object_id: self.cube.full_id,
            side,
            gltf_json,
            asset_id,
        };
        self.modify_raw(by, build_modify_material_params_request(&[update]))
            .await
    }

    /// Sends the owner's `command` and notes what it brought.
    async fn command(&mut self, command: Command) -> Result<Posted, TestFailure> {
        let sent = Instant::now();
        self.primary.send(command).await?;
        self.after(None, sent).await
    }

    /// What the request answered with `answer` left behind: what each avatar
    /// was sent, and the cube's materials as each stream then states them.
    async fn after(
        &mut self,
        answer: Option<String>,
        sent: Instant,
    ) -> Result<Posted, TestFailure> {
        let (cube, local) = (self.cube.full_id, self.cube.local_id);
        let to_owner = hear(self.primary, cube, local, self.pace.quiet, sent).await?;
        let to_other = hear(self.secondary, cube, local, self.drain, sent).await?;
        let before = self.state.clone();
        self.state.note(&to_owner);
        self.others_state.note(&to_other);
        if let Some(latest) = &to_owner.latest {
            self.cube = latest.clone();
        }
        Ok(Posted {
            answer,
            before,
            after: self.state.clone(),
            others: self.others_state.clone(),
            to_owner,
            to_other,
        })
    }

    /// Records the leg `key`: what the capability answered, what changed,
    /// what each avatar was told, and what the other avatar's stream states
    /// that the owner's does not.
    fn record(&mut self, metrics: &mut Metrics, key: &str, posted: &Posted) {
        if let Some(answer) = &posted.answer {
            metrics.set(&format!("{key}_answers"), answer.clone());
        }
        metrics.set(
            &format!("{key}_changes"),
            changes(&posted.before, &posted.after, &mut self.names),
        );
        metrics.set(&format!("{key}_told_owner"), posted.to_owner.summary());
        metrics.set(&format!("{key}_told_other"), posted.to_other.summary());
        metrics.set(
            &format!("{key}_other_sees"),
            if posted.others == posted.after {
                "what the owner does".to_owned()
            } else {
                format!(
                    "otherwise: {}",
                    changes(&posted.after, &posted.others, &mut self.names)
                )
            },
        );
        if let Some(message) = posted.to_owner.overrides.first() {
            metrics.set(&format!("{key}_override_after_ms"), millis(message.after));
        }
    }

    /// Records the leg `key` where the region granted the capability it
    /// needed.
    fn record_if(&mut self, metrics: &mut Metrics, key: &str, posted: Option<&Posted>) {
        if let Some(posted) = posted {
            self.record(metrics, key, posted);
        }
    }

    /// The other avatar asks for the cube again (`RequestMultipleObjects`),
    /// as a viewer does for an object its cache has no record of.
    async fn other_asks_again(&mut self) -> Result<Posted, TestFailure> {
        let sent = Instant::now();
        self.secondary
            .send(Command::RequestObjects {
                local_ids: vec![self.others_cube],
            })
            .await?;
        self.after(None, sent).await
    }

    /// The legacy material id face `face` of the cube now has.
    fn legacy_id(&self, face: usize) -> Option<Uuid> {
        self.state.legacy.get(face).copied().flatten()
    }

    /// The override face `face` now has, read back with the viewer's own
    /// decoder and compared with `sent`.
    fn override_round_trip(&self, face: u8, sent: &MaterialOverride) -> String {
        match self.state.overrides.get(&face) {
            None => "nothing came back".to_owned(),
            Some(document) => match parse_material_override(document.as_bytes()) {
                Ok(got) => round_trip(&override_differences(sent, &got)),
                Err(error) => format!("unreadable: {error}"),
            },
        }
    }
}

/// A census of materials on a prim: what a grid keeps, who may change it and
/// who is told.
#[derive(Debug)]
pub struct Materials;

impl GridTest for Materials {
    fn name(&self) -> &'static str {
        "materials"
    }

    fn description(&self) -> &'static str {
        "What a grid keeps of legacy and PBR materials on a prim, who may change them and who is told"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let folders = login_folders(ctx.primary()).await?;
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut residue, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            // Take the cube away whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            outcome
                .and(cleanup)
                .and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// Records every PBR metric the census holds a grid to as unanswerable: the
/// region grants no `ModifyMaterialParams`.
fn record_no_pbr(metrics: &mut Metrics) {
    for (key, _measured) in MEASURED {
        if key.starts_with("pbr_") && metrics.get(key).is_none() {
            metrics.set(key, NO_CAPABILITY);
        }
    }
}

/// Logs both avatars in at the spot, rezzes the cube and runs the legs. The
/// cube is noted in `residue` as it appears, so the caller can take it away
/// when a leg fails.
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let drain = if grid.is_fake() { FAKE_DRAIN } else { DRAIN };
    let Some((primary, secondary)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "materials needs a secondary avatar".to_owned(),
        ));
    };
    let owner = primary
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    // Both avatars to the spot, each with its scene settled.
    primary.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) = settle_scene(
        primary,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    secondary.wait_for_region(REGION_TIMEOUT).await?;
    settle_scene(
        secondary,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let mut run = Run {
        folders,
        owner,
        pace,
        position: sl_client_tokio::Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };

    // --- 1. The capabilities. ---
    for (key, name) in [
        ("render_materials_cap_granted", "RenderMaterials"),
        ("modify_cap_granted", "ModifyMaterialParams"),
        (
            "material_agent_inventory_cap_granted",
            "UpdateMaterialAgentInventory",
        ),
        (
            "material_task_inventory_cap_granted",
            "UpdateMaterialTaskInventory",
        ),
    ] {
        metrics.set(key, primary.cap(name).is_some());
    }
    let (Some(owners), Some(others)) = (Caps::of(primary), Caps::of(secondary)) else {
        return Err(TestFailure::State(
            "the region grants no RenderMaterials capability".to_owned(),
        ));
    };

    let cube = rez_cube(primary, &mut run, residue, 0).await?;
    metrics.set("cube", cube.full_id.to_string());
    let others_cube = find_object(secondary, cube.full_id).await?;
    hear(
        secondary,
        cube.full_id,
        cube.local_id,
        drain,
        Instant::now(),
    )
    .await?;
    let mut state = State::bare();
    state.note_object(&cube);
    let mut census = Census {
        http: http_client()?,
        primary,
        secondary,
        pace,
        drain,
        cube,
        others_cube,
        owners,
        others,
        others_state: state.clone(),
        state,
        names: Names::default(),
    };
    metrics.set("fresh_cube_states", census.state.summary(&mut census.names));

    reads(&census, metrics).await?;
    legacy(&mut census, metrics).await?;
    if census.owners.modify.is_some() {
        pbr(&mut census, metrics).await?;
    } else {
        record_no_pbr(metrics);
    }
    client(&mut census, metrics).await
}

/// What `RenderMaterials` answers the reads and writes it cannot serve.
async fn reads(census: &Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let url = &census.owners.render;
    let all = ask(&census.http, Verb::Get, url, String::new()).await?;
    metrics.set("legacy_all_answers", all.zipped(false));
    let unknown = census.fetch(By::Owner, &[NO_SUCH_MATERIAL]).await?;
    metrics.set("legacy_unknown_id_answers", unknown.zipped(true));
    let none = census.fetch(By::Owner, &[]).await?;
    metrics.set("legacy_no_id_answers", none.zipped(false));
    let empty = ask(&census.http, Verb::Post, url, String::new()).await?;
    metrics.set("legacy_empty_post_answers", empty.zipped(false));
    let nonsense = "<llsd><string>nonsense</string></llsd>".to_owned();
    let post = ask(&census.http, Verb::Post, url, nonsense.clone()).await?;
    metrics.set("legacy_nonsense_post_answers", post.zipped(false));
    let put = ask(&census.http, Verb::Put, url, nonsense).await?;
    metrics.set("legacy_nonsense_put_answers", put.zipped(false));
    let empty_put = ask(&census.http, Verb::Put, url, String::new()).await?;
    metrics.set("legacy_empty_put_answers", empty_put.zipped(false));
    Ok(())
}

/// The legacy-material legs.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and a window to hear its answer in"
)]
async fn legacy(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // A set, and the read back.
    let set = census.put_face(By::Owner, 0, Some(shiny())).await?;
    census.record(metrics, "legacy_set", &set);
    let id = census.legacy_id(0);
    metrics.set(
        "legacy_material_id",
        id.map_or_else(|| "none".to_owned(), |id| id.to_string()),
    );
    if let Some(id) = id {
        let read = census.fetch(By::Owner, &[id]).await?;
        metrics.set("legacy_read_answers", read.zipped(true));
        metrics.set(
            "legacy_set_round_trips",
            read.materials()
                .iter()
                .find(|entry| entry.material_id == id)
                .map_or_else(
                    || "nothing came back".to_owned(),
                    |entry| round_trip(&material_differences(&shiny(), &entry.material)),
                ),
        );
        let theirs = census.fetch(By::Other, &[id]).await?;
        metrics.set(
            "legacy_other_read_states",
            if theirs.materials() == read.materials() {
                "what the owner's does"
            } else {
                "something else"
            },
        );
        let all = ask(
            &census.http,
            Verb::Get,
            &census.owners.render,
            String::new(),
        )
        .await?;
        metrics.set(
            "legacy_all_lists_it",
            all.materials().iter().any(|entry| entry.material_id == id),
        );
        let twice = census.fetch(By::Owner, &[id, id, NO_SUCH_MATERIAL]).await?;
        metrics.set("legacy_read_twice_answers", twice.zipped(true));
    }

    let again = census.put_face(By::Owner, 0, Some(shiny())).await?;
    census.record(metrics, "legacy_same_set", &again);
    let second = census.put_face(By::Owner, 1, Some(shiny())).await?;
    census.record(metrics, "legacy_second_face_set", &second);
    let other_material = census.put_face(By::Owner, 0, Some(matte())).await?;
    census.record(metrics, "legacy_another_material_set", &other_material);
    let default = census.put_face(By::Owner, 2, Some(plain())).await?;
    census.record(metrics, "legacy_default_material_set", &default);
    metrics.set(
        "legacy_default_material_id",
        census
            .legacy_id(2)
            .map_or_else(|| "none".to_owned(), |id| id.to_string()),
    );
    let local_id = census.local();
    let two = census
        .put(
            By::Owner,
            &[
                FaceMaterialPut {
                    local_id,
                    face: 3,
                    material: Some(shiny()),
                },
                FaceMaterialPut {
                    local_id,
                    face: 4,
                    material: Some(matte()),
                },
            ],
        )
        .await?;
    census.record(metrics, "legacy_two_faces_set", &two);
    let no_face = census.put_face(By::Owner, 9, Some(shiny())).await?;
    census.record(metrics, "legacy_no_such_face_set", &no_face);
    let no_prim = census
        .put(
            By::Owner,
            &[FaceMaterialPut {
                local_id: NO_SUCH_PRIM,
                face: 0,
                material: Some(shiny()),
            }],
        )
        .await?;
    census.record(metrics, "legacy_unknown_prim_set", &no_prim);
    let nothing = census.put(By::Owner, &[]).await?;
    census.record(metrics, "legacy_no_face_set", &nothing);
    let theirs = census.put_face(By::Other, 5, Some(shiny())).await?;
    census.record(metrics, "legacy_other_set", &theirs);
    let their_clear = census.put_face(By::Other, 1, None).await?;
    census.record(metrics, "legacy_other_clear", &their_clear);

    // A clear.
    let cleared = census.put_face(By::Owner, 0, None).await?;
    census.record(metrics, "legacy_clear", &cleared);
    let bare = census.put_face(By::Owner, 0, None).await?;
    census.record(metrics, "legacy_clear_of_a_bare_face", &bare);

    // A texture entry that leaves a material id out, and one that states one.
    let mut texture_entry = decode_texture_entry(&census.cube.texture_entry, CUBE_FACES);
    let known = census.legacy_id(3);
    if let Some(face) = texture_entry.faces.get_mut(1) {
        face.material_id = None;
    }
    let dropped = census
        .command(Command::SetObjectImage {
            local_id: census.cube.scoped_id(),
            media_url: None,
            texture_entry: texture_entry.clone(),
        })
        .await?;
    census.record(metrics, "legacy_image_leaves_the_id_out", &dropped);
    if let Some(face) = texture_entry.faces.get_mut(1) {
        face.material_id = known;
    }
    let stated = census
        .command(Command::SetObjectImage {
            local_id: census.cube.scoped_id(),
            media_url: None,
            texture_entry: texture_entry.clone(),
        })
        .await?;
    census.record(metrics, "legacy_image_states_a_known_id", &stated);
    if let Some(face) = texture_entry.faces.get_mut(5) {
        face.material_id = Some(NO_SUCH_MATERIAL);
    }
    let forged = census
        .command(Command::SetObjectImage {
            local_id: census.cube.scoped_id(),
            media_url: None,
            texture_entry,
        })
        .await?;
    census.record(metrics, "legacy_image_states_an_unknown_id", &forged);
    metrics.set("legacy_states", census.state.summary(&mut census.names));
    Ok(())
}

/// The PBR legs, on a region that grants `ModifyMaterialParams`.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and a window to hear its answer in"
)]
async fn pbr(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let Some(url) = census.owners.modify.clone() else {
        return Ok(());
    };
    let blank = Some(BLANK_MATERIAL);

    // The requests a region cannot serve.
    let get = ask(&census.http, Verb::Get, &url, String::new()).await?;
    metrics.set("pbr_http_get_answers", get.modified());
    let empty = ask(&census.http, Verb::Post, &url, String::new()).await?;
    metrics.set("pbr_empty_body_answers", empty.modified());
    let map = ask(
        &census.http,
        Verb::Post,
        &url,
        "<llsd><map /></llsd>".to_owned(),
    )
    .await?;
    metrics.set("pbr_map_body_answers", map.modified());
    let no_updates = ask(
        &census.http,
        Verb::Post,
        &url,
        "<llsd><array /></llsd>".to_owned(),
    )
    .await?;
    metrics.set("pbr_no_update_answers", no_updates.modified());
    let stranger = MaterialOverrideUpdate {
        object_id: ObjectKey::from(Uuid::from_u128(0x000B_1EC7_0000_DEAD)),
        side: 0,
        gltf_json: None,
        asset_id: blank,
    };
    let unknown = census
        .modify_raw(By::Owner, build_modify_material_params_request(&[stranger]))
        .await?;
    census.record_if(metrics, "pbr_unknown_prim", unknown.as_ref());
    let bare = census.modify(By::Owner, 0, None, None).await?;
    census.record_if(metrics, "pbr_bare_update", bare.as_ref());

    // An override with no material under it, then the material.
    let tinted = encode_override_gltf_json(&tint());
    let early = census
        .modify(By::Owner, 0, None, Some(tinted.clone()))
        .await?;
    census.record_if(metrics, "pbr_override_without_a_material", early.as_ref());
    let asset = census.modify(By::Owner, 0, blank, None).await?;
    census.record_if(metrics, "pbr_asset", asset.as_ref());
    let same_asset = census.modify(By::Owner, 0, blank, None).await?;
    census.record_if(metrics, "pbr_same_asset", same_asset.as_ref());

    // An override, the same again, and another.
    let overridden = census
        .modify(By::Owner, 0, None, Some(tinted.clone()))
        .await?;
    census.record_if(metrics, "pbr_override", overridden.as_ref());
    metrics.set(
        "pbr_override_round_trips",
        census.override_round_trip(0, &tint()),
    );
    let same = census
        .modify(By::Owner, 0, None, Some(tinted.clone()))
        .await?;
    census.record_if(metrics, "pbr_same_override", same.as_ref());
    let another = census
        .modify(
            By::Owner,
            0,
            None,
            Some(encode_override_gltf_json(&metal())),
        )
        .await?;
    census.record_if(metrics, "pbr_another_override", another.as_ref());

    // A material and an override at once.
    let enriched = encode_override_gltf_json(&rich());
    let together = census.modify(By::Owner, 1, blank, Some(enriched)).await?;
    census.record_if(metrics, "pbr_asset_with_override", together.as_ref());
    metrics.set(
        "pbr_asset_with_override_round_trips",
        census.override_round_trip(1, &rich()),
    );

    // What is no override.
    let nonsense = census
        .modify(By::Owner, 0, None, Some("not gltf".to_owned()))
        .await?;
    census.record_if(metrics, "pbr_nonsense_override", nonsense.as_ref());
    let nothing_overridden = census
        .modify(
            By::Owner,
            0,
            None,
            Some(encode_override_gltf_json(&MaterialOverride::default())),
        )
        .await?;
    census.record_if(
        metrics,
        "pbr_override_of_nothing",
        nothing_overridden.as_ref(),
    );
    let no_face = census.modify(By::Owner, 9, blank, None).await?;
    census.record_if(metrics, "pbr_no_such_face", no_face.as_ref());
    let no_asset = census
        .modify(By::Owner, 2, Some(NO_SUCH_ASSET), None)
        .await?;
    census.record_if(metrics, "pbr_unknown_asset", no_asset.as_ref());

    // The other avatar.
    let theirs = census.modify(By::Other, 3, blank, None).await?;
    census.record_if(metrics, "pbr_other_asset", theirs.as_ref());
    let their_override = census
        .modify(By::Other, 1, None, Some(tinted.clone()))
        .await?;
    census.record_if(metrics, "pbr_other_override", their_override.as_ref());
    let again = census.other_asks_again().await?;
    metrics.set("pbr_asked_again_told_other", again.to_other.summary());

    // A legacy material on a face that has a PBR one.
    let both = census.put_face(By::Owner, 1, Some(matte())).await?;
    census.record(metrics, "pbr_legacy_on_a_pbr_face", &both);

    // The override cleared, the material set again and cleared.
    let cleared = census
        .modify(By::Owner, 1, None, Some(String::new()))
        .await?;
    census.record_if(metrics, "pbr_override_cleared", cleared.as_ref());
    let retinted = census
        .modify(By::Owner, 0, None, Some(tinted.clone()))
        .await?;
    census.record_if(metrics, "pbr_override_set_again", retinted.as_ref());
    let replaced = census
        .modify(By::Owner, 0, Some(NO_SUCH_ASSET), None)
        .await?;
    census.record_if(
        metrics,
        "pbr_another_asset_under_an_override",
        replaced.as_ref(),
    );
    let removed = census.modify(By::Owner, 0, Some(Uuid::nil()), None).await?;
    census.record_if(metrics, "pbr_asset_cleared", removed.as_ref());
    let after_clear = census.other_asks_again().await?;
    metrics.set(
        "pbr_asked_again_after_the_clear_told_other",
        after_clear.to_other.summary(),
    );

    // Every face at once.
    let everywhere = census.modify(By::Owner, -1, blank, None).await?;
    census.record_if(metrics, "pbr_all_faces_asset", everywhere.as_ref());
    let all_tinted = census.modify(By::Owner, -1, None, Some(tinted)).await?;
    census.record_if(metrics, "pbr_all_faces_override", all_tinted.as_ref());
    let all_removed = census
        .modify(By::Owner, -1, Some(Uuid::nil()), None)
        .await?;
    census.record_if(metrics, "pbr_all_faces_cleared", all_removed.as_ref());
    let at_the_end = census.other_asks_again().await?;
    metrics.set(
        "pbr_asked_again_at_the_end_told_other",
        at_the_end.to_other.summary(),
    );
    metrics.set("pbr_states", census.state.summary(&mut census.names));
    Ok(())
}

/// The client's own commands, on every grid: a legacy set and its fetch, and
/// a PBR edit — which a region that grants no `ModifyMaterialParams` cannot
/// take.
async fn client(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let set = census
        .command(Command::SetRenderMaterials {
            updates: vec![FaceMaterialPut {
                local_id: census.local(),
                face: 5,
                material: Some(shiny()),
            }],
        })
        .await?;
    census.record(metrics, "client_legacy_set", &set);
    let fetched = match census.legacy_id(5) {
        None => "no id to ask for".to_owned(),
        Some(id) => {
            let asked = census
                .command(Command::RequestRenderMaterials {
                    material_ids: vec![id],
                })
                .await?;
            asked.to_owner.fetched.first().map_or_else(
                || "no event".to_owned(),
                |entries| {
                    entries
                        .iter()
                        .find(|entry| entry.material_id == id)
                        .map_or_else(
                            || format!("{} material(s), not the one asked for", entries.len()),
                            |entry| round_trip(&material_differences(&shiny(), &entry.material)),
                        )
                },
            )
        }
    };
    metrics.set("client_legacy_fetch_states", fetched);

    let modified = census
        .command(Command::ModifyMaterialParams {
            updates: vec![MaterialOverrideUpdate {
                object_id: census.cube.full_id,
                side: 4,
                gltf_json: None,
                asset_id: Some(BLANK_MATERIAL),
            }],
        })
        .await?;
    census.record(metrics, "client_pbr_set", &modified);
    metrics.set(
        "client_pbr_set_result",
        modified.to_owner.results.first().map_or_else(
            || "no event".to_owned(),
            |(success, message)| {
                if message.is_empty() {
                    format!("success={success}")
                } else {
                    format!("success={success}, message={message}")
                }
            },
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BLANK_MATERIAL, Names, OverrideMessage, State, Told, changes, material_differences, matte,
        override_differences, rich, shiny, tint,
    };
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{ObjectUpdateForm, Uuid};
    use sl_material::{encode_override_gltf_json, parse_gltf_material_document};
    use std::time::Duration;

    /// A grid's ids are named in the order they were first seen, and the ids
    /// the census chose by what they are.
    #[test]
    fn ids_are_named_in_order_of_first_sight() {
        let mut names = Names::default();
        let (first, second) = (Uuid::from_u128(7), Uuid::from_u128(3));
        assert_eq!(names.of(first), "A");
        assert_eq!(names.of(second), "B");
        assert_eq!(names.of(first), "A");
        assert_eq!(names.of(BLANK_MATERIAL), "the blank material");
        assert_eq!(names.of_face(None), "none");
    }

    /// The difference between two states names the faces and what each
    /// gained or lost.
    #[test]
    fn changes_name_faces_and_fields() {
        let mut names = Names::default();
        let bare = State::bare();
        let mut set = State::bare();
        if let Some(face) = set.legacy.get_mut(0) {
            *face = Some(Uuid::from_u128(9));
        }
        if let Some(face) = set.pbr.get_mut(2) {
            *face = Some(BLANK_MATERIAL);
        }
        let _previous = set.overrides.insert(2, "{'mf':r0.25}".to_owned());
        assert_eq!(changes(&bare, &bare, &mut names), "nothing");
        assert_eq!(
            changes(&bare, &set, &mut names),
            "face 0: legacy=A; face 2: pbr=the blank material,override={'mf':r0.25}"
        );
        assert_eq!(
            changes(&set, &bare, &mut names),
            "face 0: legacy=none; face 2: pbr=none,override=none"
        );
        assert_eq!(
            set.summary(&mut names),
            "legacy 0=A; pbr 2=the blank material; overrides on 2"
        );
    }

    /// The last override message is the whole of the cube's overrides.
    #[test]
    fn the_last_override_message_replaces_the_rest() {
        let mut state = State::bare();
        let _previous = state.overrides.insert(0, "{}".to_owned());
        let told = Told {
            forms: vec![ObjectUpdateForm::Full],
            overrides: vec![OverrideMessage {
                after: Duration::ZERO,
                faces: vec![1],
                documents: vec!["{'rf':r0.5}".to_owned()],
            }],
            ..Told::default()
        };
        state.note(&told);
        assert_eq!(state.overrides.keys().copied().collect::<Vec<u8>>(), [1]);
        assert_eq!(
            told.summary(),
            "full update+1 override message(s), the last naming 1"
        );
        assert_eq!(Told::default().summary(), "nothing");
    }

    /// Two materials differ in the fields they differ in.
    #[test]
    fn material_differences_name_the_fields() {
        assert_eq!(material_differences(&shiny(), &shiny()), [""; 0]);
        assert_eq!(
            material_differences(&super::plain(), &matte()),
            ["SpecExp", "DiffuseAlphaMode", "AlphaMaskCutoff"]
        );
    }

    /// Every override the census sends is one the viewer's own GLTF reader
    /// takes, and they differ from each other.
    #[test]
    fn the_overrides_are_gltf() -> Result<(), String> {
        for over in [tint(), rich()] {
            let _material = parse_gltf_material_document(&encode_override_gltf_json(&over))
                .map_err(|error| error.to_string())?;
        }
        assert_eq!(override_differences(&tint(), &tint()), [""; 0]);
        assert_eq!(
            override_differences(&tint(), &rich()),
            [
                "textures",
                "transforms",
                "base_color",
                "emissive_factor",
                "metallic_factor",
                "roughness_factor",
                "alpha_mode",
                "alpha_cutoff",
                "double_sided"
            ]
        );
        Ok(())
    }
}
