//! A census of media on a prim: what each grid keeps per face, how it
//! versions it, who may change it, and who is told.
//!
//! A prim face may show a web page. The settings live in the region, one
//! record per face, and a viewer reads and writes them over two
//! capabilities:
//!
//! - **`ObjectMedia`**, a POST whose `verb` is `GET` (the per-face records
//!   and the object's media version) or `UPDATE` (new records for every
//!   face);
//! - **`ObjectMediaNavigate`**, a POST that moves one face to another URL.
//!
//! The region tells everybody else by sending the object again with a new
//! `MediaURL` — not a URL but the media version, `x-mv:<serial>/<agent>` —
//! and a viewer that sees a version it does not hold asks again.
//!
//! The case rezzes a cube as the primary avatar, with a second avatar
//! standing by, and asks one question a leg. After each it notes what the
//! capability answered, how the version moved and whom it names, what
//! changed in the records (read back by the owner), and what each avatar was
//! sent:
//!
//! 1. **A prim with no media**: the read, and the reads and writes a region
//!    cannot serve — an unknown prim, no prim, an unknown verb, a `GET`.
//! 2. **A set**: the records as they come back, the version, the face's
//!    media flag, the other avatar's read, the client's own fetch.
//! 3. **Navigation**: by the owner, to the same place again, to nothing, to
//!    no URL at all, of a face with no media and of a face the prim has not
//!    got; by the other avatar.
//! 4. **Permission**: navigation and a set by the other avatar while only
//!    the owner may, and while nobody may.
//! 5. **The whitelist**: navigation off it and on it, a set that leaves it,
//!    and an empty one.
//! 6. **Face counts**: more records than the prim has faces, fewer, and
//!    media on another face only.
//! 7. **The face's media flag**, cleared and set again over `ObjectImage`,
//!    and an `ObjectImage` that states another version.
//! 8. **The client's own commands**, and **a clear**.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/media.md` § Media on a prim; every run is held to it.
//!
//! `2av`, live grids only: the fake grid keeps media per session and tells
//! nobody (`server-fake-grid-object-media-region`). The cube is derezzed to
//! the Trash, also when a leg fails.

use std::time::Duration;

use sl_client_tokio::{
    Command, Event, MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MediaEntry, Object, ObjectKey,
    ObjectUpdateForm, Uuid, decode_texture_entry,
};
use sl_wire::{
    Llsd, build_object_media_get_request, build_object_media_navigate_request,
    build_object_media_update_request,
};

use crate::cases::object_edit::count;
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    rez_cube,
};
use crate::cases::task_inventory::{
    CapAnswer, DRAIN, FAKE_DRAIN, Heard, NO_CAPABILITY, OPENSIM_START, cap_get, cap_post,
    find_object, hold_to_measured, http_client, listen, login_folders, shape_of,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene};

/// How many faces the cube has.
const CUBE_FACES: usize = 6;

/// The page the media is set to.
const HOME: &str = "https://example.com/";

/// Where the owner navigates it.
const PAGE: &str = "https://example.com/page";

/// Where the other avatar navigates it.
const OTHERS_PAGE: &str = "https://example.com/theirs";

/// A page the whitelist names.
const LISTED: &str = "https://example.com/listed";

/// A page it does not.
const OFF_LIST: &str = "https://example.org/elsewhere";

/// The whitelist.
const WHITELIST: &str = "example.com";

/// Text that is no URL.
const NO_URL: &str = "not a url";

/// An address that is a URL and no web page.
const DATA_URL: &str = "data:text/plain,media";

/// A version no region minted.
const FORGED_VERSION: &str = "x-mv:0000000099/00000000-0000-0000-0000-000000000000";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/media.md § Media on a prim (object-media, 2026-10-12)";

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
        "another_face_set_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    (
        "another_face_set_changes",
        each(
            Answer::Text("face 0 lost its media; face 3 gained media at https://example.com/"),
            Answer::Text("face 0 lost its media"),
        ),
    ),
    (
        "another_face_set_states",
        each(
            Answer::Text("6 face(s), media on 3"),
            Answer::Text("6 face(s), media on none"),
        ),
    ),
    (
        "another_face_set_told_other",
        each(
            Answer::Text("full update: the version, media flag on 3"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "another_face_set_told_owner",
        each(
            Answer::Text("full update: the version, media flag on 3"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "another_face_set_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "clear_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    ("clear_changes", both(Answer::Text("face 0 lost its media"))),
    (
        "clear_get_states",
        both(Answer::Text("6 face(s), media on none")),
    ),
    (
        "clear_told_other",
        each(
            Answer::Text("full update: the version, media flag on none"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "clear_told_owner",
        each(
            Answer::Text("full update: the version, media flag on none"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    ("clear_version", both(Answer::Text("+1, names the owner"))),
    (
        "client_fetch_after_no_url_states",
        both(Answer::Text(
            "what the capability states, without the address that is no URL",
        )),
    ),
    (
        "client_fetch_states",
        both(Answer::Text("what the capability states")),
    ),
    (
        "client_navigate_changes",
        both(Answer::Text(
            "face 0: current_url=https://example.com/listed",
        )),
    ),
    (
        "client_navigate_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "client_navigate_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "client_navigate_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "client_set_changes",
        each(
            Answer::Text(
                "face 0: current_url=https://example.com/page,home_url=https://example.com/page",
            ),
            Answer::Text("face 0 gained media at https://example.com/page"),
        ),
    ),
    (
        "client_set_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "client_set_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "client_set_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "empty_body_answers",
        each(
            Answer::Text("200: error {code=8006,message=No verb}"),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "empty_get_answers",
        both(Answer::Text(
            "200: a map of object_id,object_media_data,object_media_version",
        )),
    ),
    (
        "empty_get_states",
        both(Answer::Text("6 face(s), media on none")),
    ),
    (
        "empty_get_version",
        each(
            Answer::Text(""),
            Answer::Text("x-mv:0000000000/00000000-0000-0000-0000-000000000000"),
        ),
    ),
    (
        "empty_object_states",
        both(Answer::Text("no version, media flag on none")),
    ),
    (
        "empty_whitelist_navigate_answers",
        each(Answer::Text("200: undefined"), Answer::Text("400: no body")),
    ),
    (
        "empty_whitelist_navigate_changes",
        each(
            Answer::Text("face 0: current_url=https://example.com/page"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "empty_whitelist_navigate_told_other",
        each(
            Answer::Text("full update: the version, media flag on 0"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "empty_whitelist_navigate_told_owner",
        each(
            Answer::Text("full update: the version, media flag on 0"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "empty_whitelist_navigate_version",
        each(
            Answer::Text("+1, names the owner"),
            Answer::Text("unchanged"),
        ),
    ),
    (
        "get_answers",
        both(Answer::Text(
            "200: a map of object_id,object_media_data,object_media_version",
        )),
    ),
    ("get_bare_face_shape", both(Answer::Text("undef"))),
    (
        "get_entry_shape",
        each(
            Answer::Text(
                "{alt_image_enable:boolean,auto_loop:boolean,auto_play:boolean,auto_scale:boolean,auto_zoom:boolean,controls:integer,current_url:string,first_click_interact:boolean,height_pixels:integer,home_url:string,perms_control:integer,perms_interact:integer,whitelist_enable:boolean,width_pixels:integer}",
            ),
            Answer::Text(
                "{alt_image_enable:boolean,auto_loop:boolean,auto_play:boolean,auto_scale:boolean,auto_zoom:boolean,controls:integer,current_url:string,first_click_interact:boolean,height_pixels:integer,home_url:string,perms_control:integer,perms_interact:integer,whitelist:[0],whitelist_enable:boolean,width_pixels:integer}",
            ),
        ),
    ),
    (
        "get_of_an_unknown_prim_answers",
        each(Answer::Text("404: no body"), Answer::Text("400: no body")),
    ),
    (
        "get_of_no_prim_answers",
        each(
            Answer::Text("200: error {code=8005,message=No object_id}"),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "http_get_answers",
        each(Answer::Text("503: no body"), Answer::Text("400: no body")),
    ),
    (
        "image_clears_the_flag_changes",
        both(Answer::Text("face 0 lost its media")),
    ),
    (
        "image_clears_the_flag_told_other",
        both(Answer::Text(
            "terse update: the version, media flag on none",
        )),
    ),
    (
        "image_clears_the_flag_told_owner",
        both(Answer::Text(
            "terse update: the version, media flag on none",
        )),
    ),
    (
        "image_clears_the_flag_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "image_sets_the_flag_changes",
        each(
            Answer::Text("face 0 gained media at "),
            Answer::Text("nothing"),
        ),
    ),
    (
        "image_sets_the_flag_told_other",
        both(Answer::Text("terse update: the version, media flag on 0")),
    ),
    (
        "image_sets_the_flag_told_owner",
        both(Answer::Text("terse update: the version, media flag on 0")),
    ),
    (
        "image_sets_the_flag_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "image_states_another_version_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "image_states_another_version_object_says",
        both(Answer::Text("the region's version")),
    ),
    (
        "image_states_another_version_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "image_states_another_version_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "image_states_another_version_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "listed_navigate_answers",
        both(Answer::Text("200: undefined")),
    ),
    (
        "listed_navigate_changes",
        both(Answer::Text(
            "face 0: current_url=https://example.com/listed",
        )),
    ),
    (
        "listed_navigate_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "listed_navigate_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "listed_navigate_version",
        both(Answer::Text("+1, names the owner")),
    ),
    ("media_cap_granted", both(Answer::Flag(true))),
    (
        "navigate_a_bare_face_answers",
        each(Answer::Text("404: no body"), Answer::Text("400: no body")),
    ),
    (
        "navigate_a_bare_face_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_a_bare_face_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_a_bare_face_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_a_bare_face_version",
        both(Answer::Text("unchanged")),
    ),
    ("navigate_answers", both(Answer::Text("200: undefined"))),
    ("navigate_cap_granted", both(Answer::Flag(true))),
    (
        "navigate_changes",
        both(Answer::Text("face 0: current_url=https://example.com/page")),
    ),
    (
        "navigate_http_get_answers",
        each(Answer::Text("405: no body"), Answer::Text("400: no body")),
    ),
    (
        "navigate_no_such_face_answers",
        each(Answer::Text("404: no body"), Answer::Text("400: no body")),
    ),
    (
        "navigate_no_such_face_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_no_such_face_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_no_such_face_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_no_such_face_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "navigate_of_an_unknown_prim_answers",
        each(Answer::Text("404: no body"), Answer::Text("400: no body")),
    ),
    (
        "navigate_to_a_data_url_answers",
        both(Answer::Text("200: undefined")),
    ),
    (
        "navigate_to_a_data_url_changes",
        both(Answer::Text("face 0: current_url=data:text/plain,media")),
    ),
    (
        "navigate_to_a_data_url_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "navigate_to_a_data_url_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "navigate_to_a_data_url_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "navigate_to_no_url_answers",
        both(Answer::Text("200: undefined")),
    ),
    (
        "navigate_to_no_url_changes",
        each(
            Answer::Text("face 0: current_url=https://not a url"),
            Answer::Text("face 0: current_url=not a url"),
        ),
    ),
    (
        "navigate_to_no_url_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "navigate_to_no_url_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "navigate_to_no_url_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "navigate_to_nothing_answers",
        each(Answer::Text("404: no body"), Answer::Text("200: undefined")),
    ),
    (
        "navigate_to_nothing_changes",
        each(
            Answer::Text("nothing"),
            Answer::Text("face 0: current_url="),
        ),
    ),
    (
        "navigate_to_nothing_told_other",
        each(
            Answer::Text("nothing"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "navigate_to_nothing_told_owner",
        each(
            Answer::Text("nothing"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "navigate_to_nothing_version",
        each(
            Answer::Text("unchanged"),
            Answer::Text("+1, names the owner"),
        ),
    ),
    (
        "navigate_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "navigate_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "navigate_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "navigate_without_media_answers",
        each(Answer::Text("404: no body"), Answer::Text("400: no body")),
    ),
    (
        "navigate_without_media_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_without_media_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_without_media_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "navigate_without_media_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "no_face_set_answers",
        each(
            Answer::Text(
                "200: error {code=8001,message=Bad input: incorrect number of texture indices}",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    ("no_face_set_changes", both(Answer::Text("nothing"))),
    ("no_face_set_told_other", both(Answer::Text("nothing"))),
    ("no_face_set_told_owner", both(Answer::Text("nothing"))),
    ("no_face_set_version", both(Answer::Text("unchanged"))),
    (
        "off_list_navigate_answers",
        each(
            Answer::Text(
                "200: error {code=8003,message=navigate (https://example.org/elsewhere) did not match whitelist domains}",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    ("off_list_navigate_changes", both(Answer::Text("nothing"))),
    (
        "off_list_navigate_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "off_list_navigate_told_owner",
        both(Answer::Text("nothing")),
    ),
    ("off_list_navigate_version", both(Answer::Text("unchanged"))),
    (
        "off_list_set_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    (
        "off_list_set_changes",
        both(Answer::Text(
            "face 0: current_url=https://example.org/elsewhere,home_url=https://example.org/elsewhere",
        )),
    ),
    (
        "off_list_set_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "off_list_set_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "off_list_set_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "one_face_set_answers",
        each(
            Answer::Text(
                "200: error {code=8001,message=Bad input: incorrect number of texture indices}",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "one_face_set_changes",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "face 0: current_url=https://example.com/page,home_url=https://example.com/page",
            ),
        ),
    ),
    ("one_face_set_told_other", both(Answer::Text("nothing"))),
    ("one_face_set_told_owner", both(Answer::Text("nothing"))),
    ("one_face_set_version", both(Answer::Text("unchanged"))),
    (
        "other_get_states",
        both(Answer::Text("what the owner's does")),
    ),
    (
        "other_navigate_answers",
        both(Answer::Text("200: undefined")),
    ),
    (
        "other_navigate_changes",
        both(Answer::Text(
            "face 0: current_url=https://example.com/theirs",
        )),
    ),
    (
        "other_navigate_owner_only_answers",
        each(
            Answer::Text(
                "200: error {code=8002,message=agent <id>: navigate not allowed on object <id>}",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "other_navigate_owner_only_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "other_navigate_owner_only_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "other_navigate_owner_only_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "other_navigate_owner_only_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "other_navigate_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "other_navigate_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "other_navigate_version",
        both(Answer::Text("+1, names the other avatar")),
    ),
    (
        "other_off_list_navigate_answers",
        each(
            Answer::Text(
                "200: error {code=8003,message=navigate (https://example.org/elsewhere) did not match whitelist domains}",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "other_off_list_navigate_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "other_off_list_navigate_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "other_off_list_navigate_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "other_off_list_navigate_version",
        both(Answer::Text("unchanged")),
    ),
    (
        "other_set_answers",
        each(
            Answer::Text(
                "200: error {code=8002,message=Agent <id> does not have edit permission on object <id>}",
            ),
            Answer::Text("200: no body"),
        ),
    ),
    ("other_set_changes", both(Answer::Text("nothing"))),
    (
        "other_set_owner_only_answers",
        each(
            Answer::Text(
                "200: error {code=8002,message=Agent <id> does not have edit permission on object <id>}",
            ),
            Answer::Text("200: no body"),
        ),
    ),
    (
        "other_set_owner_only_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "other_set_owner_only_told_other",
        each(
            Answer::Text("nothing"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "other_set_owner_only_told_owner",
        each(
            Answer::Text("nothing"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "other_set_owner_only_version",
        each(
            Answer::Text("unchanged"),
            Answer::Text("+1, names the other avatar"),
        ),
    ),
    (
        "other_set_told_other",
        each(
            Answer::Text("nothing"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "other_set_told_owner",
        each(
            Answer::Text("nothing"),
            Answer::Text("full update: the version, media flag on 0"),
        ),
    ),
    (
        "other_set_version",
        each(
            Answer::Text("unchanged"),
            Answer::Text("+1, names the other avatar"),
        ),
    ),
    (
        "owner_navigate_nobody_may_answers",
        each(Answer::Text("200: undefined"), Answer::Text("400: no body")),
    ),
    (
        "owner_navigate_nobody_may_changes",
        each(
            Answer::Text("face 0: current_url=https://example.com/page"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "owner_navigate_nobody_may_told_other",
        each(
            Answer::Text("full update: the version, media flag on 0"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "owner_navigate_nobody_may_told_owner",
        each(
            Answer::Text("full update: the version, media flag on 0"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "owner_navigate_nobody_may_version",
        each(
            Answer::Text("+1, names the owner"),
            Answer::Text("unchanged"),
        ),
    ),
    (
        "owner_navigate_owner_only_answers",
        both(Answer::Text("200: undefined")),
    ),
    (
        "owner_navigate_owner_only_changes",
        both(Answer::Text("face 0: current_url=https://example.com/page")),
    ),
    (
        "owner_navigate_owner_only_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "owner_navigate_owner_only_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "owner_navigate_owner_only_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "owner_only_set_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    (
        "owner_only_set_changes",
        both(Answer::Text(
            "face 0: current_url=https://example.com/,perms_interact,perms_control",
        )),
    ),
    (
        "owner_only_set_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "owner_only_set_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "owner_only_set_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "owner_set_nobody_may_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    (
        "owner_set_nobody_may_changes",
        each(
            Answer::Text("face 0: current_url=https://example.com/,perms_interact,perms_control"),
            Answer::Text("face 0: perms_interact,perms_control"),
        ),
    ),
    (
        "owner_set_nobody_may_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "owner_set_nobody_may_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "owner_set_nobody_may_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "same_navigate_answers",
        both(Answer::Text("200: undefined")),
    ),
    ("same_navigate_changes", both(Answer::Text("nothing"))),
    (
        "same_navigate_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "same_navigate_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "same_navigate_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "same_set_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    ("same_set_changes", both(Answer::Text("nothing"))),
    (
        "same_set_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "same_set_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "same_set_version",
        both(Answer::Text("+1, names the owner")),
    ),
    (
        "set_answers",
        each(Answer::Text("200: undefined"), Answer::Text("200: no body")),
    ),
    (
        "set_changes",
        both(Answer::Text("face 0 gained media at https://example.com/")),
    ),
    (
        "set_get_states",
        both(Answer::Text("6 face(s), media on 0")),
    ),
    ("set_round_trips", both(Answer::Text("every field"))),
    (
        "set_told_other",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    (
        "set_told_owner",
        both(Answer::Text("full update: the version, media flag on 0")),
    ),
    ("set_version", both(Answer::Text("+1, names the owner"))),
    (
        "seven_face_set_answers",
        each(
            Answer::Text(
                "200: error {code=8001,message=Bad input: incorrect number of texture indices}",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    ("seven_face_set_changes", both(Answer::Text("nothing"))),
    ("seven_face_set_told_other", both(Answer::Text("nothing"))),
    ("seven_face_set_told_owner", both(Answer::Text("nothing"))),
    ("seven_face_set_version", both(Answer::Text("unchanged"))),
    ("texture_entry_faces", both(Answer::Count(6))),
    (
        "unknown_verb_answers",
        each(
            Answer::Text("200: error {code=8006,message=Invalid verb}"),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "version_format",
        both(Answer::Text("x-mv:<10 digits>/<id>")),
    ),
];

/// Which avatar asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The cube's owner.
    Owner,
    /// The avatar standing by.
    Other,
}

/// One avatar's two capabilities.
#[derive(Debug, Clone)]
struct Caps {
    /// `ObjectMedia`.
    media: String,
    /// `ObjectMediaNavigate`.
    navigate: String,
}

impl Caps {
    /// The two capabilities `session` was granted, if it was granted both.
    fn of(session: &Session) -> Option<Self> {
        Some(Self {
            media: session.cap("ObjectMedia")?,
            navigate: session.cap("ObjectMediaNavigate")?,
        })
    }
}

/// A media version, `x-mv:<serial>/<agent>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Version {
    /// The serial.
    serial: i64,
    /// How many digits it is written in.
    digits: usize,
    /// The id after the slash.
    who: Uuid,
}

impl Version {
    /// `text` as a version, where it is one.
    fn parse(text: &str) -> Option<Self> {
        let (serial, who) = text.strip_prefix("x-mv:")?.split_once('/')?;
        Some(Self {
            serial: serial.parse().ok()?,
            digits: serial.len(),
            who: Uuid::parse_str(who).ok()?,
        })
    }
}

/// One face's media record, with its two addresses as the grid wrote them: a
/// grid may keep text that is no URL, which a [`MediaEntry`] cannot hold.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Face {
    /// The record, its addresses left out.
    entry: MediaEntry,
    /// `current_url`, as written.
    current_url: String,
    /// `home_url`, as written.
    home_url: String,
}

impl Face {
    /// The record `entry` as a grid would state it.
    fn sent(entry: &MediaEntry) -> Self {
        let text =
            |url: &Option<url::Url>| url.as_ref().map_or_else(String::new, ToString::to_string);
        Self {
            current_url: text(&entry.current_url),
            home_url: text(&entry.home_url),
            entry: MediaEntry {
                current_url: None,
                home_url: None,
                ..entry.clone()
            },
        }
    }

    /// The record the LLSD map `value` states, where its other fields read.
    fn of(value: &Llsd) -> Option<Self> {
        let Llsd::Map(map) = value else {
            return None;
        };
        let text = |key: &str| {
            map.get(key)
                .and_then(Llsd::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let mut rest = map.clone();
        rest.remove("current_url");
        rest.remove("home_url");
        Some(Self {
            entry: MediaEntry::from_llsd(&Llsd::Map(rest)).ok()?,
            current_url: text("current_url"),
            home_url: text("home_url"),
        })
    }
}

/// A prim's media as an `ObjectMedia` `GET` states it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct State {
    /// The version, as written.
    version: String,
    /// The record of each face.
    faces: Vec<Option<Face>>,
}

impl State {
    /// The state `answer` states, or none where it is no listing.
    fn of(answer: &CapAnswer) -> Self {
        let Some(body) = answer.body.as_ref() else {
            return Self::default();
        };
        Self {
            version: body
                .get("object_media_version")
                .and_then(Llsd::as_str)
                .unwrap_or_default()
                .to_owned(),
            faces: body
                .get("object_media_data")
                .and_then(Llsd::as_array)
                .map(|faces| faces.iter().map(Face::of).collect())
                .unwrap_or_default(),
        }
    }

    /// The state with every address that is no URL left out, which is all a
    /// client that holds addresses as URLs can state of it.
    fn as_a_client_holds_it(&self) -> Self {
        let kept = |text: &str| {
            text.parse::<url::Url>()
                .map_or_else(|_error| String::new(), |url| url.to_string())
        };
        Self {
            version: self.version.clone(),
            faces: self
                .faces
                .iter()
                .map(|face| {
                    face.as_ref().map(|face| Face {
                        entry: face.entry.clone(),
                        current_url: kept(&face.current_url),
                        home_url: kept(&face.home_url),
                    })
                })
                .collect(),
        }
    }

    /// The state the client's event states.
    fn of_event(version: &str, faces: &[Option<MediaEntry>]) -> Self {
        Self {
            version: version.to_owned(),
            faces: faces
                .iter()
                .map(|face| face.as_ref().map(Face::sent))
                .collect(),
        }
    }

    /// The faces that have media, `,`-joined, or `none`.
    fn media_faces(&self) -> String {
        let faces: Vec<String> = self
            .faces
            .iter()
            .enumerate()
            .filter(|(_index, face)| face.is_some())
            .map(|(index, _face)| index.to_string())
            .collect();
        if faces.is_empty() {
            "none".to_owned()
        } else {
            faces.join(",")
        }
    }

    /// The record of face `index`.
    fn face(&self, index: usize) -> Option<&Face> {
        self.faces.get(index).and_then(Option::as_ref)
    }

    /// The state as one metric: how many faces it lists and which have media.
    fn summary(&self) -> String {
        format!(
            "{} face(s), media on {}",
            self.faces.len(),
            self.media_faces()
        )
    }
}

/// The fields `left` and `right` differ in.
fn entry_differences(left: &Face, right: &Face) -> Vec<&'static str> {
    let (left_face, right_face) = (left, right);
    let (left, right) = (&left.entry, &right.entry);
    [
        (
            "alt_image_enable",
            left.alt_image_enable == right.alt_image_enable,
        ),
        ("controls", left.controls == right.controls),
        (
            "current_url",
            left_face.current_url == right_face.current_url,
        ),
        ("home_url", left_face.home_url == right_face.home_url),
        ("auto_loop", left.auto_loop == right.auto_loop),
        ("auto_play", left.auto_play == right.auto_play),
        ("auto_scale", left.auto_scale == right.auto_scale),
        ("auto_zoom", left.auto_zoom == right.auto_zoom),
        (
            "first_click_interact",
            left.first_click_interact == right.first_click_interact,
        ),
        ("width_pixels", left.width_pixels == right.width_pixels),
        ("height_pixels", left.height_pixels == right.height_pixels),
        (
            "whitelist_enable",
            left.whitelist_enable == right.whitelist_enable,
        ),
        ("whitelist", left.whitelist == right.whitelist),
        (
            "perms_interact",
            left.perms_interact == right.perms_interact,
        ),
        ("perms_control", left.perms_control == right.perms_control),
    ]
    .into_iter()
    .filter(|(_name, same)| !same)
    .map(|(name, _same)| name)
    .collect()
}

/// What changed between two states, face by face — or `nothing`.
fn changes(before: &State, after: &State) -> String {
    let mut parts = Vec::new();
    if before.faces.len() != after.faces.len() {
        parts.push(format!(
            "{} face(s) became {}",
            before.faces.len(),
            after.faces.len()
        ));
    }
    for index in 0..before.faces.len().max(after.faces.len()) {
        match (before.face(index), after.face(index)) {
            (None, Some(face)) => {
                parts.push(format!("face {index} gained media at {}", face.current_url));
            }
            (Some(_), None) => parts.push(format!("face {index} lost its media")),
            (Some(old), Some(new)) => {
                let fields = entry_differences(old, new);
                if !fields.is_empty() {
                    let fields: Vec<String> = fields
                        .into_iter()
                        .map(|field| match field {
                            "current_url" => format!("current_url={}", new.current_url),
                            "home_url" => format!("home_url={}", new.home_url),
                            other => other.to_owned(),
                        })
                        .collect();
                    parts.push(format!("face {index}: {}", fields.join(",")));
                }
            }
            (None, None) => {}
        }
    }
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("; ")
    }
}

/// An LLSD value as text: a scalar as written, a map's keys in order.
pub(crate) fn rendered(value: &Llsd) -> String {
    match value {
        Llsd::Map(map) => {
            let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
            entries.sort_unstable_by_key(|(key, _value)| *key);
            let entries: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{key}={}", rendered(value)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Llsd::Array(items) => {
            let items: Vec<String> = items.iter().map(rendered).collect();
            format!("[{}]", items.join(","))
        }
        Llsd::Undef => "undef".to_owned(),
        Llsd::Boolean(flag) => flag.to_string(),
        Llsd::Integer(number) => number.to_string(),
        Llsd::Real(number) => number.to_string(),
        // An error names the agent and the prim; a metric must not.
        Llsd::String(text) => text
            .split(' ')
            .map(|word| {
                let (id, colon) = word.strip_suffix(':').map_or((word, ""), |id| (id, ":"));
                if Uuid::parse_str(id).is_ok() {
                    format!("<id>{colon}")
                } else {
                    word.to_owned()
                }
            })
            .collect::<Vec<String>>()
            .join(" "),
        Llsd::Uuid(_) => "<id>".to_owned(),
        Llsd::Uri(uri) => uri.clone(),
        Llsd::Binary(_) | Llsd::Date(_) => shape_of(value),
    }
}

/// What a capability answered, as a metric records it: the status and the
/// kind of body, and an error spelt out.
pub(crate) fn answered(answer: &CapAnswer) -> String {
    match answer.body.as_ref().and_then(|body| body.get("error")) {
        Some(error) => format!("{}: error {}", answer.status, rendered(error)),
        None => answer.shape(),
    }
}

/// What one request brought.
struct Posted {
    /// What the capability answered, where the request went to one.
    answer: Option<CapAnswer>,
    /// The media before.
    before: State,
    /// The media after.
    after: State,
    /// What the owner was sent.
    to_owner: Heard,
    /// What the other avatar was sent.
    to_other: Heard,
}

/// A media entry at `home`, showing `current`, with the reference viewer's
/// defaults.
fn entry(home: &str, current: &str) -> Result<MediaEntry, TestFailure> {
    let parse = |text: &str| {
        text.parse::<url::Url>()
            .map_err(|error| TestFailure::Assertion(format!("{text} is no URL: {error}")))
    };
    Ok(MediaEntry {
        home_url: Some(parse(home)?),
        current_url: Some(parse(current)?),
        ..MediaEntry::default()
    })
}

/// `count` faces with `entry` on face `index` and no media elsewhere.
fn on_face(entry: &MediaEntry, index: usize, count: usize) -> Vec<Option<MediaEntry>> {
    (0..count)
        .map(|face| (face == index).then(|| entry.clone()))
        .collect()
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
    /// The owner.
    owner: Uuid,
    /// The other avatar.
    other: Uuid,
    /// The owner's capabilities.
    owners: Caps,
    /// The other avatar's.
    others: Caps,
    /// The cube's media as last read.
    state: State,
}

impl Census<'_> {
    /// The capabilities of `by`.
    const fn caps(&self, by: By) -> &Caps {
        match by {
            By::Owner => &self.owners,
            By::Other => &self.others,
        }
    }

    /// An `ObjectMedia` `GET` of the cube by `by`.
    async fn read(&self, by: By) -> Result<CapAnswer, TestFailure> {
        cap_post(
            &self.http,
            &self.caps(by).media,
            build_object_media_get_request(self.cube.full_id),
        )
        .await
    }

    /// POSTs `body` to the `ObjectMedia` capability of `by`.
    async fn media(&mut self, by: By, body: String) -> Result<Posted, TestFailure> {
        let answer = cap_post(&self.http, &self.caps(by).media, body).await?;
        self.after(Some(answer)).await
    }

    /// An `UPDATE` of the cube to `faces` by `by`.
    async fn set(&mut self, by: By, faces: &[Option<MediaEntry>]) -> Result<Posted, TestFailure> {
        self.media(
            by,
            build_object_media_update_request(self.cube.full_id, faces),
        )
        .await
    }

    /// A navigation of `face` of the cube to `url` by `by`.
    async fn navigate(&mut self, by: By, face: u8, url: &str) -> Result<Posted, TestFailure> {
        let body = build_object_media_navigate_request(self.cube.full_id, face, url);
        let answer = cap_post(&self.http, &self.caps(by).navigate, body).await?;
        self.after(Some(answer)).await
    }

    /// Sends the owner's `command` and notes what it brought.
    async fn command(&mut self, command: Command) -> Result<Posted, TestFailure> {
        self.primary.send(command).await?;
        self.after(None).await
    }

    /// What the request `answer` came back from left behind: what each avatar
    /// was sent, and the media read back by the owner.
    async fn after(&mut self, answer: Option<CapAnswer>) -> Result<Posted, TestFailure> {
        let to_owner = listen(self.primary, self.pace.quiet).await?;
        let to_other = listen(self.secondary, self.drain).await?;
        let before = self.state.clone();
        self.state = State::of(&self.read(By::Owner).await?);
        if let Some(latest) = to_owner
            .changed
            .iter()
            .rfind(|object| object.full_id == self.cube.full_id)
        {
            self.cube = latest.clone();
        }
        Ok(Posted {
            answer,
            before,
            after: self.state.clone(),
            to_owner,
            to_other,
        })
    }

    /// Whom the version `who` names.
    fn who(&self, who: Uuid) -> &'static str {
        if who.is_nil() {
            "nobody"
        } else if who == self.owner {
            "the owner"
        } else if who == self.other {
            "the other avatar"
        } else if who == self.cube.full_id.uuid() {
            "the prim"
        } else {
            "somebody else"
        }
    }

    /// How the version moved from `before` to `after`, and whom it names.
    fn version_step(&self, before: &str, after: &str) -> String {
        if before == after {
            return "unchanged".to_owned();
        }
        match Version::parse(after) {
            Some(version) => format!(
                "{:+}, names {}",
                version
                    .serial
                    .saturating_sub(Version::parse(before).map_or(0, |version| version.serial)),
                self.who(version.who)
            ),
            None if after.is_empty() => "gone".to_owned(),
            None => format!("became {after}"),
        }
    }

    /// How `heard` told of the cube: the forms it was sent in, the media
    /// version the last one stated and the faces whose media flag it set — or
    /// `nothing`.
    fn told(&self, heard: &Heard) -> String {
        let forms: Vec<&str> = [
            (ObjectUpdateForm::Full, "full update"),
            (ObjectUpdateForm::Compressed, "compressed update"),
            (ObjectUpdateForm::Terse, "terse update"),
        ]
        .into_iter()
        .filter(|(form, _word)| heard.updated.contains(&(self.cube.local_id, *form)))
        .map(|(_form, word)| word)
        .collect();
        if forms.is_empty() {
            return "nothing".to_owned();
        }
        let detail = heard
            .changed
            .iter()
            .rfind(|object| object.full_id == self.cube.full_id)
            .map_or_else(String::new, |object| {
                format!(": {}", self.stream_says(object))
            });
        format!("{}{detail}", forms.join("+"))
    }

    /// What `object`, the cube as the stream sent it, says of its media: the
    /// version in its `MediaURL` and the faces whose media flag is set.
    fn stream_says(&self, object: &Object) -> String {
        let version = match object.media_url.as_ref().map(url::Url::as_str) {
            None => "no version",
            Some(version) if version == self.state.version => "the version",
            Some(_) => "another version",
        };
        let flagged: Vec<String> = decode_texture_entry(&object.texture_entry, CUBE_FACES)
            .faces
            .iter()
            .enumerate()
            .filter(|(_index, face)| face.media_enabled())
            .map(|(index, _face)| index.to_string())
            .collect();
        format!(
            "{version}, media flag on {}",
            if flagged.is_empty() {
                "none".to_owned()
            } else {
                flagged.join(",")
            }
        )
    }

    /// Records the leg `key`: what the capability answered, how the version
    /// moved, what changed, and what each avatar was told.
    fn record(&self, metrics: &mut Metrics, key: &str, posted: &Posted) {
        if let Some(answer) = &posted.answer {
            metrics.set(&format!("{key}_answers"), answered(answer));
        }
        metrics.set(
            &format!("{key}_version"),
            self.version_step(&posted.before.version, &posted.after.version),
        );
        metrics.set(
            &format!("{key}_changes"),
            changes(&posted.before, &posted.after),
        );
        metrics.set(&format!("{key}_told_owner"), self.told(&posted.to_owner));
        metrics.set(&format!("{key}_told_other"), self.told(&posted.to_other));
    }

    /// The client's own fetch of the cube's media: whether the event states
    /// what the capability does.
    async fn client_fetch(&mut self) -> Result<String, TestFailure> {
        let object_id = self.cube.full_id;
        self.primary
            .send(Command::RequestObjectMedia { object_id })
            .await?;
        let answer = self
            .primary
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ObjectMedia {
                    object_id: of,
                    version,
                    faces,
                } if *of == object_id => Some(State::of_event(version, faces)),
                _ => None,
            })
            .await;
        match answer {
            Ok(state) if state == self.state => Ok("what the capability states".to_owned()),
            Ok(state) if state == self.state.as_a_client_holds_it() => {
                Ok("what the capability states, without the address that is no URL".to_owned())
            }
            Ok(state) => Ok(format!(
                "something else: {}, {}",
                state.version,
                state.summary()
            )),
            Err(TestFailure::Timeout(_)) => Ok("no event".to_owned()),
            Err(other) => Err(other),
        }
    }
}

/// A census of media on a prim: the records, the version, who may change
/// them and who is told.
#[derive(Debug)]
pub struct ObjectMedia;

impl GridTest for ObjectMedia {
    fn name(&self) -> &'static str {
        "object-media"
    }

    fn description(&self) -> &'static str {
        "What a grid keeps of media on a prim, how it versions it, who may change it and who is told"
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

/// The metrics a region that grants no media capability is recorded under.
fn record_no_capability(metrics: &mut Metrics) {
    for (key, _measured) in MEASURED {
        if !matches!(*key, "media_cap_granted" | "navigate_cap_granted") {
            metrics.set(key, NO_CAPABILITY);
        }
    }
}

/// The legs, in order. The cube is noted in `residue` as it appears, so the
/// caller can take it away when a leg fails.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of eight legs, each a request and a window to hear its answer in"
)]
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
            "object-media needs a secondary avatar".to_owned(),
        ));
    };
    let owner = primary
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let other = secondary.agent_id().ok_or_else(|| {
        TestFailure::Assertion("the second login reported no agent id".to_owned())
    })?;

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

    let owners = Caps::of(primary);
    let others = Caps::of(secondary);
    metrics.set("media_cap_granted", primary.cap("ObjectMedia").is_some());
    metrics.set(
        "navigate_cap_granted",
        primary.cap("ObjectMediaNavigate").is_some(),
    );
    let (Some(owners), Some(others)) = (owners, others) else {
        record_no_capability(metrics);
        return Ok(());
    };

    let cube = rez_cube(primary, &mut run, residue, 0).await?;
    metrics.set("cube", cube.full_id.to_string());
    find_object(secondary, cube.full_id).await?;
    listen(secondary, drain).await?;
    let http = http_client()?;
    let mut census = Census {
        http,
        primary,
        secondary,
        pace,
        drain,
        cube,
        owner: owner.uuid(),
        other: other.uuid(),
        owners,
        others,
        state: State::default(),
    };

    // --- 1. A prim with no media. ---
    let empty = census.read(By::Owner).await?;
    census.state = State::of(&empty);
    metrics.set("empty_get_answers", empty.shape());
    metrics.set("empty_get_version", census.state.version.clone());
    metrics.set("empty_get_states", census.state.summary());
    metrics.set("empty_object_states", census.stream_says(&census.cube));
    let nobody = ObjectKey::from(Uuid::from_u128(0x000B_1EC7_0000_DEAD));
    let unknown = cap_post(
        &census.http,
        &census.owners.media,
        build_object_media_get_request(nobody),
    )
    .await?;
    metrics.set("get_of_an_unknown_prim_answers", answered(&unknown));
    let no_prim = cap_post(
        &census.http,
        &census.owners.media,
        "<llsd><map><key>verb</key><string>GET</string></map></llsd>".to_owned(),
    )
    .await?;
    metrics.set("get_of_no_prim_answers", answered(&no_prim));
    let no_verb = cap_post(
        &census.http,
        &census.owners.media,
        format!(
            "<llsd><map><key>verb</key><string>PURGE</string><key>object_id</key><uuid>{}</uuid></map></llsd>",
            census.cube.full_id.uuid()
        ),
    )
    .await?;
    metrics.set("unknown_verb_answers", answered(&no_verb));
    let nothing = cap_post(&census.http, &census.owners.media, String::new()).await?;
    metrics.set("empty_body_answers", answered(&nothing));
    let http_get = cap_get(&census.http, &census.owners.media).await?;
    metrics.set("http_get_answers", answered(&http_get));
    let navigate_get = cap_get(&census.http, &census.owners.navigate).await?;
    metrics.set("navigate_http_get_answers", answered(&navigate_get));
    let unknown_navigate = cap_post(
        &census.http,
        &census.owners.navigate,
        build_object_media_navigate_request(nobody, 0, PAGE),
    )
    .await?;
    metrics.set(
        "navigate_of_an_unknown_prim_answers",
        answered(&unknown_navigate),
    );
    let bare = census.navigate(By::Owner, 0, PAGE).await?;
    census.record(metrics, "navigate_without_media", &bare);

    // --- 2. A set. ---
    let home = entry(HOME, HOME)?;
    let set = census
        .set(By::Owner, &on_face(&home, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "set", &set);
    metrics.set("set_get_states", census.state.summary());
    metrics.set("set_version_text", census.state.version.clone());
    metrics.set(
        "version_format",
        Version::parse(&census.state.version).map_or_else(
            || "no version".to_owned(),
            |version| format!("x-mv:<{} digits>/<id>", version.digits),
        ),
    );
    metrics.set(
        "set_round_trips",
        census.state.face(0).map_or_else(
            || "nothing came back".to_owned(),
            |got| {
                let fields = entry_differences(&Face::sent(&home), got);
                if fields.is_empty() {
                    "every field".to_owned()
                } else {
                    format!("but for {}", fields.join(","))
                }
            },
        ),
    );
    let read = census.read(By::Owner).await?;
    metrics.set("get_answers", read.shape());
    let listed = read
        .body
        .as_ref()
        .and_then(|body| body.get("object_media_data"))
        .and_then(Llsd::as_array);
    metrics.set(
        "get_entry_shape",
        listed
            .and_then(<[Llsd]>::first)
            .map_or_else(|| "no entry".to_owned(), shape_of),
    );
    metrics.set(
        "get_bare_face_shape",
        listed
            .and_then(|faces| faces.get(1))
            .map_or_else(|| "no entry".to_owned(), shape_of),
    );
    let theirs = State::of(&census.read(By::Other).await?);
    metrics.set(
        "other_get_states",
        if theirs == census.state {
            "what the owner's does".to_owned()
        } else {
            format!("something else: {}, {}", theirs.version, theirs.summary())
        },
    );
    metrics.set("client_fetch_states", census.client_fetch().await?);
    let again = census
        .set(By::Owner, &on_face(&home, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "same_set", &again);

    // --- 3. Navigation. ---
    let navigated = census.navigate(By::Owner, 0, PAGE).await?;
    census.record(metrics, "navigate", &navigated);
    let same = census.navigate(By::Owner, 0, PAGE).await?;
    census.record(metrics, "same_navigate", &same);
    let to_nothing = census.navigate(By::Owner, 0, "").await?;
    census.record(metrics, "navigate_to_nothing", &to_nothing);
    let to_no_url = census.navigate(By::Owner, 0, NO_URL).await?;
    census.record(metrics, "navigate_to_no_url", &to_no_url);
    metrics.set(
        "client_fetch_after_no_url_states",
        census.client_fetch().await?,
    );
    let to_data = census.navigate(By::Owner, 0, DATA_URL).await?;
    census.record(metrics, "navigate_to_a_data_url", &to_data);
    let bare_face = census.navigate(By::Owner, 1, PAGE).await?;
    census.record(metrics, "navigate_a_bare_face", &bare_face);
    let no_face = census.navigate(By::Owner, 9, PAGE).await?;
    census.record(metrics, "navigate_no_such_face", &no_face);
    let theirs = census.navigate(By::Other, 0, OTHERS_PAGE).await?;
    census.record(metrics, "other_navigate", &theirs);

    // --- 4. Permission. ---
    let owner_only = MediaEntry {
        perms_interact: MEDIA_PERM_OWNER,
        perms_control: MEDIA_PERM_OWNER,
        ..entry(HOME, HOME)?
    };
    let narrowed = census
        .set(By::Owner, &on_face(&owner_only, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "owner_only_set", &narrowed);
    let refused = census.navigate(By::Other, 0, OTHERS_PAGE).await?;
    census.record(metrics, "other_navigate_owner_only", &refused);
    let allowed = census.navigate(By::Owner, 0, PAGE).await?;
    census.record(metrics, "owner_navigate_owner_only", &allowed);
    let strangers = entry(OTHERS_PAGE, OTHERS_PAGE)?;
    let stranger_set = census
        .set(By::Other, &on_face(&strangers, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "other_set_owner_only", &stranger_set);
    let nobody_may = MediaEntry {
        perms_interact: MEDIA_PERM_NONE,
        perms_control: MEDIA_PERM_NONE,
        ..entry(HOME, HOME)?
    };
    census
        .set(By::Owner, &on_face(&nobody_may, 0, CUBE_FACES))
        .await?;
    let owner_refused = census.navigate(By::Owner, 0, PAGE).await?;
    census.record(metrics, "owner_navigate_nobody_may", &owner_refused);
    let owner_resets = census
        .set(By::Owner, &on_face(&home, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "owner_set_nobody_may", &owner_resets);
    let stranger_set = census
        .set(By::Other, &on_face(&strangers, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "other_set", &stranger_set);

    // --- 5. The whitelist. ---
    let whitelisted = MediaEntry {
        whitelist_enable: true,
        whitelist: vec![WHITELIST.to_owned()],
        ..entry(HOME, HOME)?
    };
    census
        .set(By::Owner, &on_face(&whitelisted, 0, CUBE_FACES))
        .await?;
    let off_list = census.navigate(By::Owner, 0, OFF_LIST).await?;
    census.record(metrics, "off_list_navigate", &off_list);
    let on_list = census.navigate(By::Owner, 0, LISTED).await?;
    census.record(metrics, "listed_navigate", &on_list);
    let others_off_list = census.navigate(By::Other, 0, OFF_LIST).await?;
    census.record(metrics, "other_off_list_navigate", &others_off_list);
    let leaves = MediaEntry {
        whitelist_enable: true,
        whitelist: vec![WHITELIST.to_owned()],
        ..entry(OFF_LIST, OFF_LIST)?
    };
    let off_list_set = census
        .set(By::Owner, &on_face(&leaves, 0, CUBE_FACES))
        .await?;
    census.record(metrics, "off_list_set", &off_list_set);
    let no_list = MediaEntry {
        whitelist_enable: true,
        ..entry(HOME, HOME)?
    };
    census
        .set(By::Owner, &on_face(&no_list, 0, CUBE_FACES))
        .await?;
    let empty_list = census.navigate(By::Owner, 0, PAGE).await?;
    census.record(metrics, "empty_whitelist_navigate", &empty_list);

    // --- 6. Face counts. ---
    census
        .set(By::Owner, &on_face(&home, 0, CUBE_FACES))
        .await?;
    let page = entry(PAGE, PAGE)?;
    let too_many = census
        .set(By::Owner, &on_face(&page, 0, CUBE_FACES.saturating_add(1)))
        .await?;
    census.record(metrics, "seven_face_set", &too_many);
    let too_few = census.set(By::Owner, &on_face(&page, 0, 1)).await?;
    census.record(metrics, "one_face_set", &too_few);
    let elsewhere = census
        .set(By::Owner, &on_face(&home, 3, CUBE_FACES))
        .await?;
    census.record(metrics, "another_face_set", &elsewhere);
    metrics.set("another_face_set_states", census.state.summary());

    // --- 7. The face's media flag. ---
    census
        .set(By::Owner, &on_face(&home, 0, CUBE_FACES))
        .await?;
    let version = census
        .cube
        .media_url
        .as_ref()
        .map(|url| url.as_str().to_owned());
    let mut texture_entry = decode_texture_entry(&census.cube.texture_entry, CUBE_FACES);
    metrics.set("texture_entry_faces", count(texture_entry.faces.len()));
    for face in &mut texture_entry.faces {
        face.media_flags &= !1;
    }
    let unflagged = census
        .command(Command::SetObjectImage {
            local_id: census.cube.scoped_id(),
            media_url: version.clone(),
            texture_entry: texture_entry.clone(),
        })
        .await?;
    census.record(metrics, "image_clears_the_flag", &unflagged);
    if let Some(face) = texture_entry.faces.first_mut() {
        face.media_flags |= 1;
    }
    let reflagged = census
        .command(Command::SetObjectImage {
            local_id: census.cube.scoped_id(),
            media_url: census
                .cube
                .media_url
                .as_ref()
                .map(|url| url.as_str().to_owned()),
            texture_entry: texture_entry.clone(),
        })
        .await?;
    census.record(metrics, "image_sets_the_flag", &reflagged);
    let forged = census
        .command(Command::SetObjectImage {
            local_id: census.cube.scoped_id(),
            media_url: Some(FORGED_VERSION.to_owned()),
            texture_entry,
        })
        .await?;
    census.record(metrics, "image_states_another_version", &forged);
    metrics.set(
        "image_states_another_version_object_says",
        census.cube.media_url.as_ref().map_or("no version", |url| {
            if url.as_str() == FORGED_VERSION {
                "the version the image stated"
            } else {
                "the region's version"
            }
        }),
    );

    // --- 8. The client's own commands, and a clear. ---
    let object_id = census.cube.full_id;
    let by_command = census
        .command(Command::SetObjectMedia {
            object_id,
            faces: on_face(&page, 0, CUBE_FACES),
        })
        .await?;
    census.record(metrics, "client_set", &by_command);
    let by_command = census
        .command(Command::NavigateObjectMedia {
            object_id,
            face: 0,
            url: LISTED.to_owned(),
        })
        .await?;
    census.record(metrics, "client_navigate", &by_command);
    let cleared = census.set(By::Owner, &on_face(&home, 0, 0)).await?;
    census.record(metrics, "no_face_set", &cleared);
    let cleared = census.set(By::Owner, &vec![None; CUBE_FACES]).await?;
    census.record(metrics, "clear", &cleared);
    metrics.set("clear_get_states", census.state.summary());
    metrics.set("clear_version_text", census.state.version.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Face, State, Version, changes, entry, on_face};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;

    /// A version is `x-mv:`, a zero-padded serial, a slash and an id.
    #[test]
    fn a_version_parses() {
        let version = Version::parse("x-mv:0000000012/00000000-0000-0000-0000-000000000000");
        assert_eq!(
            version,
            Some(Version {
                serial: 12,
                digits: 10,
                who: Uuid::nil(),
            })
        );
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("https://example.com/"), None);
    }

    /// The difference between two states names the faces and the fields.
    #[test]
    fn changes_name_faces_and_fields() -> Result<(), String> {
        let home = entry(super::HOME, super::HOME).map_err(|failure| failure.to_string())?;
        let page = entry(super::HOME, super::PAGE).map_err(|failure| failure.to_string())?;
        let empty = State {
            version: String::new(),
            faces: vec![None; 2],
        };
        let set = State {
            version: String::new(),
            faces: on_face(&home, 0, 2)
                .iter()
                .map(|face| face.as_ref().map(Face::sent))
                .collect(),
        };
        let navigated = State {
            version: String::new(),
            faces: on_face(&page, 0, 2)
                .iter()
                .map(|face| face.as_ref().map(Face::sent))
                .collect(),
        };
        assert_eq!(changes(&empty, &empty), "nothing");
        assert_eq!(
            changes(&empty, &set),
            "face 0 gained media at https://example.com/"
        );
        assert_eq!(
            changes(&set, &navigated),
            "face 0: current_url=https://example.com/page"
        );
        assert_eq!(changes(&set, &empty), "face 0 lost its media");
        assert_eq!(
            changes(&set, &State::default()),
            "2 face(s) became 0; face 0 lost its media"
        );
        Ok(())
    }
}
