//! A census of the asset-delivery capabilities as HTTP: what each grid's
//! `GetTexture`, `GetMesh`, `GetMesh2` and `ViewerAsset` answer a request
//! with — the status, the body and the headers — rather than whether a fetch
//! through the client's stores succeeds (`texture-fetch-http`,
//! `mesh-fetch-http` and `asset-fetch-http` hold that).
//!
//! The case asks each capability itself, with a client that follows no
//! redirect and decodes nothing, about one asset it knows the grid has — the
//! plywood texture, a mesh a prim in view is made of, a body part the avatar
//! wears — and asks one question a request:
//!
//! 1. **The whole asset**: the status, the media type and the headers that
//!    tell a cache what it may do.
//! 2. **Ranges**: the first bytes, the first byte alone, a middle, an open
//!    end, an end past the asset, the whole asset by range, a start at and
//!    past the end, a suffix, two ranges, a backwards one, one that is no
//!    number and one in another unit.
//! 3. **Conditions**: the entity tag and the date the grid stated, sent back.
//! 4. **Other requests**: a `HEAD`, a `POST`, another `Accept`, a compressed
//!    answer asked for, the URL without the slash the reference writes.
//! 5. **What is not there**: an id no grid has an asset under, with and
//!    without a range; the nil id; text that is no id; no query; a query key
//!    the capability does not know; an asset of another class.
//!
//! An offset is written against the asset's length `N`, so that one answer
//! reads the same for a 600-byte asset as for a 600,000-byte one.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/assets.md`; every run is held to it. A `502`, `503` or
//! `504` is asked again a few times before it is taken for the answer, and
//! how often that happened is recorded beside the answers, not held.

use std::time::Duration;

use reqwest::Method;
use reqwest::header::{
    HeaderMap as ReqwestHeaderMap, HeaderName as ReqwestHeaderName,
    HeaderValue as ReqwestHeaderValue,
};
use sl_client_tokio::{BUILTIN_UI_SOUNDS, Command, Event, Throttle, Uuid};

use crate::cases::mesh_fetch_http::scan_for_mesh;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{NO_CAPABILITY, OPENSIM_START, hold_to_measured};
use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, fixtures, is_opensim};

/// An asset id no grid has an asset under.
const NO_SUCH_ASSET: Uuid = Uuid::from_u128(0x0000_0bad_a55e_7000_8000_0000_0000_dead);

/// The shortest asset the range questions can be asked of: the ranges name
/// bytes up to the thirtieth, and an offset within ten of the end is written
/// against the end.
const SHORTEST_ASSET: usize = 64;

/// How often a `502`, `503` or `504` is asked again before it is the answer.
const TRANSIENT_RETRIES: u32 = 5;

/// How long to wait before asking again after a transient status.
const TRANSIENT_PAUSE: Duration = Duration::from_secs(2);

/// What a capability with no asset to ask about records for every question.
const NO_ASSET: &str = "no asset known";

/// What a capability that answered none of its assets whole records for every
/// question after the first.
const NO_ANSWER: &str = "no asset answered";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/assets.md (asset-http, 2026-10-12)";

/// The questions asked of every capability, by the suffix of the metric each
/// is recorded under.
const QUESTIONS: &[&str] = &[
    "url",
    "whole",
    "whole_headers",
    "range_first",
    "range_first_byte",
    "range_middle",
    "range_open",
    "range_end_past",
    "range_whole",
    "range_wider",
    "range_start_at_end",
    "range_start_past",
    "range_open_at_end",
    "range_suffix",
    "range_two",
    "range_backwards",
    "range_no_number",
    "range_other_unit",
    "if_none_match",
    "if_modified_since",
    "head",
    "post",
    "other_accept",
    "gzip",
    "no_slash",
    "missing",
    "missing_range",
    "nil_id",
    "no_id",
    "no_query",
    "other_key",
    "other_class",
];

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

/// The questions a Second-Life-flavoured fake grid is not held to: what aditi
/// answers them with is its proxy failing to reach its store, or a header a
/// viewer may refuse — neither of which the fake grid
/// imitates (`ImitatedGrid::asset_http_policy` says why).
const NOT_IMITATED_AS_SECOND_LIFE: &[&str] = &[
    "_missing",
    "_missing_range",
    "_nil_id",
    "_no_id",
    "_no_query",
    "_other_key",
    "_other_class",
    "_post",
    "_whole_encoding",
];

/// The questions neither fake flavour is held to: the fake grid's store does
/// not keep an asset's class, so it cannot say what class another
/// capability's asset is.
const NOT_IMITATED: &[&str] = &["_other_class_as"];

/// What each grid was measured to answer, by the metric the census records it
/// under. Every run is held to all of it at the end, so a run that disagrees
/// says everything it disagrees about at once.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "one_url_for_all",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "asset_url_shape",
        each(
            Answer::Text("the root of its host"),
            Answer::Text("a path with a token"),
        ),
    ),
    (
        "mesh_url_shape",
        each(
            Answer::Text("the root of its host"),
            Answer::Text("a path with a token"),
        ),
    ),
    (
        "mesh2_url_shape",
        each(
            Answer::Text("the root of its host"),
            Answer::Text("a path with a token"),
        ),
    ),
    (
        "texture_url_shape",
        each(
            Answer::Text("the root of its host"),
            Answer::Text("a path with a token"),
        ),
    ),
    (
        "asset_asked_under",
        each(Answer::Text("sound_id"), Answer::Text("bodypart_id")),
    ),
    ("asset_gzip", both(Answer::Text("200: as without it"))),
    ("asset_head", both(Answer::Text("200: content-length N"))),
    (
        "asset_if_modified_since",
        each(
            Answer::Text("304: no body; no content-range"),
            Answer::Text("no last-modified stated"),
        ),
    ),
    (
        "asset_if_none_match",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("no etag stated"),
        ),
    ),
    (
        "asset_missing",
        each(
            Answer::Text(
                "503: application/ogg \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "asset_missing_range",
        each(
            Answer::Text(
                "503: application/ogg \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "asset_nil_id",
        each(
            Answer::Text(
                "503: application/ogg \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "asset_no_id",
        each(
            Answer::Text(
                "503: application/ogg \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "asset_no_query",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "asset_no_slash",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "asset_of_a_mesh",
        both(Answer::Text("200: some bytes as application/vnd.ll.mesh")),
    ),
    (
        "asset_of_a_texture",
        both(Answer::Text("200: some bytes as image/x-j2c")),
    ),
    (
        "asset_other_accept",
        each(
            Answer::Text("200: the asset as application/ogg"),
            Answer::Text("200: the asset as application/vnd.ll.bodypart"),
        ),
    ),
    (
        "asset_other_class",
        each(
            Answer::Text(
                "503: application/vnd.ll.bodypart \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"; no content-range",
            ),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    ("asset_other_class_as", both(Answer::Text("nothing"))),
    (
        "asset_other_key",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "asset_post",
        each(
            Answer::Text(
                "411: application/ogg \"Bad Request Bad Request Your browser sent a request that this server could not u\"; no content-range",
            ),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "asset_range_backwards",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("206: bytes 10-10; content-range bytes 10-10/N"),
        ),
    ),
    (
        "asset_range_end_past",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "asset_range_first",
        both(Answer::Text("206: bytes 0-9; content-range bytes 0-9/N")),
    ),
    (
        "asset_range_first_byte",
        each(
            Answer::Text("206: bytes 0-0; content-range bytes 0-0/N"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "asset_range_middle",
        both(Answer::Text(
            "206: bytes 10-19; content-range bytes 10-19/N",
        )),
    ),
    (
        "asset_range_no_number",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "asset_range_open",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "asset_range_open_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "asset_range_other_unit",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "asset_range_start_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "asset_range_start_past",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "asset_range_suffix",
        each(
            Answer::Text("206: bytes N-10-N-1; content-range bytes N-10-N-1/N"),
            Answer::Text("206: bytes 0-10; content-range bytes 0-10/N"),
        ),
    ),
    (
        "asset_range_two",
        each(
            Answer::Text("206: application/ogg, a multipart body; no content-range"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "asset_range_whole",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "asset_range_wider",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "asset_url",
        each(
            Answer::Text("http, another host or port"),
            Answer::Text("http, the simulator's host and port"),
        ),
    ),
    (
        "asset_whole",
        each(
            Answer::Text("200: the asset as application/ogg"),
            Answer::Text("200: the asset as application/vnd.ll.bodypart"),
        ),
    ),
    (
        "asset_whole_encoding",
        each(
            Answer::Text("binary/octet-stream"),
            Answer::Text("not stated"),
        ),
    ),
    (
        "asset_whole_headers",
        each(
            Answer::Text("accept-ranges: bytes; etag; last-modified"),
            Answer::Text("none"),
        ),
    ),
    ("mesh2_asked_under", both(Answer::Text("mesh_id"))),
    ("mesh2_gzip", both(Answer::Text("200: as without it"))),
    ("mesh2_head", both(Answer::Text("200: content-length N"))),
    (
        "mesh2_if_modified_since",
        each(
            Answer::Text("304: no body; no content-range"),
            Answer::Text("no last-modified stated"),
        ),
    ),
    (
        "mesh2_if_none_match",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("no etag stated"),
        ),
    ),
    (
        "mesh2_missing",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "mesh2_missing_range",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "mesh2_nil_id",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "mesh2_no_id",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "mesh2_no_query",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "mesh2_no_slash",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh2_other_accept",
        both(Answer::Text("200: the asset as application/vnd.ll.mesh")),
    ),
    (
        "mesh2_other_class",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("200: some bytes"),
        ),
    ),
    (
        "mesh2_other_class_as",
        each(Answer::Text("nothing"), Answer::Text("image/x-j2c")),
    ),
    (
        "mesh2_other_key",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "mesh2_post",
        each(
            Answer::Text(
                "411: application/vnd.ll.mesh \"Bad Request Bad Request Your browser sent a request that this server could not u\"; no content-range",
            ),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "mesh2_range_backwards",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("206: bytes 10-10; content-range bytes 10-10/N"),
        ),
    ),
    (
        "mesh2_range_end_past",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "mesh2_range_first",
        both(Answer::Text("206: bytes 0-9; content-range bytes 0-9/N")),
    ),
    (
        "mesh2_range_first_byte",
        each(
            Answer::Text("206: bytes 0-0; content-range bytes 0-0/N"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "mesh2_range_middle",
        both(Answer::Text(
            "206: bytes 10-19; content-range bytes 10-19/N",
        )),
    ),
    (
        "mesh2_range_no_number",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh2_range_open",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "mesh2_range_open_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh2_range_other_unit",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh2_range_start_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh2_range_start_past",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh2_range_suffix",
        each(
            Answer::Text("206: bytes N-10-N-1; content-range bytes N-10-N-1/N"),
            Answer::Text("206: bytes 0-10; content-range bytes 0-10/N"),
        ),
    ),
    (
        "mesh2_range_two",
        each(
            Answer::Text("206: application/vnd.ll.mesh, a multipart body; no content-range"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "mesh2_range_whole",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "mesh2_range_wider",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "mesh2_url",
        each(
            Answer::Text("http, another host or port"),
            Answer::Text("http, the simulator's host and port"),
        ),
    ),
    (
        "mesh2_whole",
        both(Answer::Text("200: the asset as application/vnd.ll.mesh")),
    ),
    (
        "mesh2_whole_encoding",
        each(
            Answer::Text("binary/octet-stream"),
            Answer::Text("not stated"),
        ),
    ),
    (
        "mesh2_whole_headers",
        each(
            Answer::Text("accept-ranges: bytes; etag; last-modified"),
            Answer::Text("none"),
        ),
    ),
    ("mesh_asked_under", both(Answer::Text("mesh_id"))),
    ("mesh_gzip", both(Answer::Text("200: as without it"))),
    ("mesh_head", both(Answer::Text("200: content-length N"))),
    (
        "mesh_if_modified_since",
        each(
            Answer::Text("304: no body; no content-range"),
            Answer::Text("no last-modified stated"),
        ),
    ),
    (
        "mesh_if_none_match",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("no etag stated"),
        ),
    ),
    (
        "mesh_missing",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "mesh_missing_range",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "mesh_nil_id",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "mesh_no_id",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "mesh_no_query",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "mesh_no_slash",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh_other_accept",
        both(Answer::Text("200: the asset as application/vnd.ll.mesh")),
    ),
    (
        "mesh_other_class",
        each(
            Answer::Text(
                "503: application/vnd.ll.mesh \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("200: some bytes"),
        ),
    ),
    (
        "mesh_other_class_as",
        each(Answer::Text("nothing"), Answer::Text("image/x-j2c")),
    ),
    (
        "mesh_other_key",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "mesh_post",
        each(
            Answer::Text(
                "411: application/vnd.ll.mesh \"Bad Request Bad Request Your browser sent a request that this server could not u\"; no content-range",
            ),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "mesh_range_backwards",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("206: bytes 10-10; content-range bytes 10-10/N"),
        ),
    ),
    (
        "mesh_range_end_past",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "mesh_range_first",
        both(Answer::Text("206: bytes 0-9; content-range bytes 0-9/N")),
    ),
    (
        "mesh_range_first_byte",
        each(
            Answer::Text("206: bytes 0-0; content-range bytes 0-0/N"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "mesh_range_middle",
        both(Answer::Text(
            "206: bytes 10-19; content-range bytes 10-19/N",
        )),
    ),
    (
        "mesh_range_no_number",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh_range_open",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "mesh_range_open_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh_range_other_unit",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh_range_start_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh_range_start_past",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "mesh_range_suffix",
        each(
            Answer::Text("206: bytes N-10-N-1; content-range bytes N-10-N-1/N"),
            Answer::Text("206: bytes 0-10; content-range bytes 0-10/N"),
        ),
    ),
    (
        "mesh_range_two",
        each(
            Answer::Text("206: application/vnd.ll.mesh, a multipart body; no content-range"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "mesh_range_whole",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "mesh_range_wider",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "mesh_url",
        each(
            Answer::Text("http, another host or port"),
            Answer::Text("http, the simulator's host and port"),
        ),
    ),
    (
        "mesh_whole",
        both(Answer::Text("200: the asset as application/vnd.ll.mesh")),
    ),
    (
        "mesh_whole_encoding",
        each(
            Answer::Text("binary/octet-stream"),
            Answer::Text("not stated"),
        ),
    ),
    (
        "mesh_whole_headers",
        each(
            Answer::Text("accept-ranges: bytes; etag; last-modified"),
            Answer::Text("none"),
        ),
    ),
    ("redirects", both(Answer::Count(0))),
    ("texture_asked_under", both(Answer::Text("texture_id"))),
    ("texture_gzip", both(Answer::Text("200: as without it"))),
    ("texture_head", both(Answer::Text("200: content-length N"))),
    (
        "texture_if_modified_since",
        each(
            Answer::Text("304: no body; no content-range"),
            Answer::Text("no last-modified stated"),
        ),
    ),
    (
        "texture_if_none_match",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("no etag stated"),
        ),
    ),
    (
        "texture_missing",
        each(
            Answer::Text(
                "503: image/x-j2c \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "texture_missing_range",
        each(
            Answer::Text(
                "503: image/x-j2c \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "texture_nil_id",
        each(
            Answer::Text("200: image/x-j2c, 5854 bytes"),
            Answer::Text("404: no body"),
        ),
    ),
    (
        "texture_no_id",
        each(
            Answer::Text(
                "503: image/x-j2c \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "texture_no_query",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "texture_no_slash",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "texture_other_accept",
        both(Answer::Text("200: the asset as image/x-j2c")),
    ),
    (
        "texture_other_class",
        each(
            Answer::Text(
                "503: image/x-j2c \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("200: some bytes"),
        ),
    ),
    (
        "texture_other_class_as",
        each(
            Answer::Text("nothing"),
            Answer::Text("application/vnd.ll.mesh"),
        ),
    ),
    (
        "texture_other_key",
        each(
            Answer::Text(
                "503: text/html \"Service Unavailable Service Unavailable - DNS failure The server is temporarily \"",
            ),
            Answer::Text("400: no body"),
        ),
    ),
    (
        "texture_post",
        each(
            Answer::Text(
                "411: image/x-j2c \"Bad Request Bad Request Your browser sent a request that this server could not u\"; no content-range",
            ),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "texture_range_backwards",
        each(
            Answer::Text("200: the whole asset; no content-range"),
            Answer::Text("206: bytes 10-10; content-range bytes 10-10/N"),
        ),
    ),
    (
        "texture_range_end_past",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "texture_range_first",
        both(Answer::Text("206: bytes 0-9; content-range bytes 0-9/N")),
    ),
    (
        "texture_range_first_byte",
        each(
            Answer::Text("206: bytes 0-0; content-range bytes 0-0/N"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "texture_range_middle",
        both(Answer::Text(
            "206: bytes 10-19; content-range bytes 10-19/N",
        )),
    ),
    (
        "texture_range_no_number",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "texture_range_open",
        both(Answer::Text(
            "206: bytes 10-N-1; content-range bytes 10-N-1/N",
        )),
    ),
    (
        "texture_range_open_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "texture_range_other_unit",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "texture_range_start_at_end",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "texture_range_start_past",
        both(Answer::Text("200: the whole asset; no content-range")),
    ),
    (
        "texture_range_suffix",
        each(
            Answer::Text("206: bytes N-10-N-1; content-range bytes N-10-N-1/N"),
            Answer::Text("206: bytes 0-10; content-range bytes 0-10/N"),
        ),
    ),
    (
        "texture_range_two",
        each(
            Answer::Text("206: image/x-j2c, a multipart body; no content-range"),
            Answer::Text("200: the whole asset; no content-range"),
        ),
    ),
    (
        "texture_range_whole",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "texture_range_wider",
        both(Answer::Text(
            "206: the whole asset; content-range bytes 0-N-1/N",
        )),
    ),
    (
        "texture_url",
        each(
            Answer::Text("http, another host or port"),
            Answer::Text("http, the simulator's host and port"),
        ),
    ),
    (
        "texture_whole",
        both(Answer::Text("200: the asset as image/x-j2c")),
    ),
    (
        "texture_whole_encoding",
        each(
            Answer::Text("binary/octet-stream"),
            Answer::Text("not stated"),
        ),
    ),
    (
        "texture_whole_headers",
        each(
            Answer::Text("accept-ranges: bytes; etag; last-modified"),
            Answer::Text("none"),
        ),
    ),
];

/// A census of the asset-delivery capabilities as HTTP.
#[derive(Debug)]
pub struct AssetHttp;

impl GridTest for AssetHttp {
    fn name(&self) -> &'static str {
        "asset-http"
    }

    fn description(&self) -> &'static str {
        "What the asset capabilities answer as HTTP: statuses, ranges, headers, what is not there"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
            let grid = ctx.grid();
            let fixture_mesh = ctx.mesh_asset();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The mesh first: the scan reads the object stream, which the
            // wait for the wearables would otherwise have read past.
            let mesh = match fixture_mesh {
                Some(mesh) => Some(mesh.uuid()),
                None => scan_for_mesh(session).await?.map(|mesh| mesh.uuid()),
            };
            // What `ViewerAsset` is asked about: what the avatar wears, then
            // the sounds every viewer plays, the first of them it answers.
            session.send(Command::RequestWearables).await?;
            let worn = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::AgentWearables { wearables, .. } => Some(wearables.clone()),
                    _other => None,
                })
                .await?;
            let worn: Vec<(&'static str, Uuid)> = worn
                .iter()
                .filter_map(|wearable| {
                    let asset = wearable.asset_id.filter(|asset| !asset.is_nil())?;
                    Some((
                        if wearable.wearable_type.is_body_part() {
                            "bodypart_id"
                        } else {
                            "clothing_id"
                        },
                        asset,
                    ))
                })
                .collect();
            let sounds = BUILTIN_UI_SOUNDS
                .into_iter()
                .map(|sound| ("sound_id", sound));
            // Aditi's asset host answers next to nothing the avatar wears
            // (see `asset_worn_answered`), and a stock OpenSim has none of
            // the sounds: each grid is asked first about what it has.
            let generic: Vec<(&'static str, Uuid)> =
                if matches!(grid.behaves_like(), sl_fake_grid::ImitatedGrid::OpenSim) {
                    worn.iter().copied().chain(sounds).collect()
                } else {
                    sounds.chain(worn.iter().copied()).collect()
                };
            let texture = fixtures::plywood_texture().uuid();

            let simulator = session
                .cap("EventQueueGet")
                .and_then(|url| authority_of(&url));
            let targets = [
                Target {
                    prefix: "texture",
                    cap: session.cap("GetTexture"),
                    assets: vec![("texture_id", texture)],
                    accept: "image/x-j2c",
                    other_class: mesh,
                },
                Target {
                    prefix: "mesh",
                    cap: session.cap("GetMesh"),
                    assets: mesh.map(|mesh| ("mesh_id", mesh)).into_iter().collect(),
                    accept: "application/vnd.ll.mesh",
                    other_class: Some(texture),
                },
                Target {
                    prefix: "mesh2",
                    cap: session.cap("GetMesh2"),
                    assets: mesh.map(|mesh| ("mesh_id", mesh)).into_iter().collect(),
                    accept: "application/vnd.ll.mesh",
                    other_class: Some(texture),
                },
                Target {
                    prefix: "asset",
                    cap: session.cap("ViewerAsset"),
                    assets: generic,
                    accept: "application/octet-stream",
                    other_class: Some(texture),
                },
            ];

            let mut asker = Asker::new()?;
            let mut metrics = Metrics::new();
            let mut unknown = Vec::new();
            let mut outcome = Ok(());
            for target in &targets {
                match census(&mut asker, target, simulator.as_deref(), &mut metrics).await {
                    Ok(true) => {}
                    Ok(false) => unknown.push(target.prefix),
                    Err(failure) => {
                        outcome = Err(failure);
                        break;
                    }
                }
            }
            // `ViewerAsset` asked for the other classes' assets, which have a
            // capability of their own.
            let viewer_asset = session.cap("ViewerAsset");
            for (name, key, asset) in [
                ("asset_of_a_texture", "texture_id", Some(texture)),
                ("asset_of_a_mesh", "mesh_id", mesh),
            ] {
                let answer = match (&viewer_asset, asset) {
                    (None, _) => NO_CAPABILITY.to_owned(),
                    (Some(_), None) => NO_ASSET.to_owned(),
                    (Some(cap), Some(asset)) => {
                        let url = format!("{cap}/?{key}={asset}");
                        match asker.get_once(&url, &[]).await {
                            Ok(whole) => whole.plain_or_bytes(),
                            Err(failure) => {
                                outcome = outcome.and(Err(failure));
                                continue;
                            }
                        }
                    }
                };
                metrics.set(name, answer);
            }
            // What the avatar wears, asked for one by one: how many of them
            // the grid's asset host answers.
            if let Some(cap) = &viewer_asset {
                let mut answered = 0_usize;
                let mut refusal = "none".to_owned();
                for (key, asset) in &worn {
                    match asker.get_once(&format!("{cap}/?{key}={asset}"), &[]).await {
                        Ok(reply) if reply.status == 200 => answered = answered.saturating_add(1),
                        Ok(reply) => refusal = reply.plain(),
                        Err(failure) => outcome = outcome.and(Err(failure)),
                    }
                }
                metrics.set(
                    "asset_worn_answered",
                    format!("{answered} of {}", worn.len()),
                );
                metrics.set("asset_worn_not_answered_with", refusal);
            }
            // Whether the four capabilities are one URL.
            let urls: std::collections::BTreeSet<&String> = targets
                .iter()
                .filter_map(|target| target.cap.as_ref())
                .collect();
            metrics.set("one_url_for_all", urls.len() == 1);
            metrics.set("transient_answers", i64::from(asker.transient));
            metrics.set("transient_answer", asker.first_transient.clone());
            metrics.set("redirects", i64::from(asker.redirects));

            ctx.metrics().merge(metrics.clone());
            outcome?;
            if !unknown.is_empty() {
                ctx.mark_partial(&format!(
                    "no asset to ask about, or none answered: {}",
                    unknown.join(", ")
                ));
                return Ok(());
            }
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| {
                    let left_out =
                        |suffixes: &[&str]| suffixes.iter().any(|suffix| key.ends_with(suffix));
                    !(grid.is_fake() && left_out(NOT_IMITATED)
                        || grid == Grid::FakeSl && left_out(NOT_IMITATED_AS_SECOND_LIFE))
                })
                .copied()
                .collect();
            hold_to_measured(grid, &metrics, &held, SOURCE)
        })
    }
}

/// One capability and the asset the census asks it about.
#[derive(Debug)]
struct Target {
    /// What the capability's metrics begin with.
    prefix: &'static str,
    /// The capability's URL, where the region granted it.
    cap: Option<String>,
    /// Assets of the capability's class the grid should have, each with the
    /// query key the capability takes it under. The first the capability
    /// answers whole is the one the census asks about.
    assets: Vec<(&'static str, Uuid)>,
    /// The media type a viewer asks the capability for.
    accept: &'static str,
    /// An asset of a class the capability is not for.
    other_class: Option<Uuid>,
}

/// What a capability answered one request with.
#[derive(Debug)]
struct Reply {
    /// The HTTP status.
    status: u16,
    /// The headers.
    headers: ReqwestHeaderMap,
    /// The body, as sent.
    body: Vec<u8>,
}

impl Reply {
    /// The value of the header `name`, where it is text.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The media type the answer states, without its parameters.
    fn media_type(&self) -> String {
        self.header("content-type").map_or_else(
            || "no media type".to_owned(),
            |value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .to_ascii_lowercase()
            },
        )
    }

    /// The answer as a metric records it, read against the `whole` asset: the
    /// status, what the body is, and the range the answer states.
    fn against(&self, whole: &[u8]) -> String {
        let total = whole.len();
        let stated = self.header("content-range");
        let range = stated.and_then(parse_content_range);
        let body = if self.body.is_empty() {
            "no body".to_owned()
        } else if self.body == whole {
            "the whole asset".to_owned()
        } else {
            match range {
                Some((Some((first, last)), _total))
                    if whole.get(first..=last) == Some(self.body.as_slice()) =>
                {
                    format!("bytes {}-{}", offset(first, total), offset(last, total))
                }
                _other => self.something_else(),
            }
        };
        let stated = match (stated, range) {
            (None, _) => "no content-range".to_owned(),
            (Some(_), Some((span, stated_total))) => {
                let span = span.map_or_else(
                    || "*".to_owned(),
                    |(first, last)| format!("{}-{}", offset(first, total), offset(last, total)),
                );
                let of = match stated_total {
                    Some(stated_total) if stated_total == total => "N".to_owned(),
                    Some(stated_total) => stated_total.to_string(),
                    None => "*".to_owned(),
                };
                format!("content-range bytes {span}/{of}")
            }
            (Some(text), None) => format!("content-range {text}"),
        };
        format!("{}: {body}; {stated}", self.status)
    }

    /// The answer as a metric records it where no asset is expected: the
    /// status and what came with it.
    fn plain(&self) -> String {
        if self.body.is_empty() {
            format!("{}: no body", self.status)
        } else {
            format!("{}: {}", self.status, self.something_else())
        }
    }

    /// The answer as a metric records it where the body, if there is one, is
    /// an asset the census has not read: the status and whether bytes came.
    fn plain_or_bytes(&self) -> String {
        if self.status == 200 || self.status == 206 {
            format!(
                "{}: {} bytes as {}",
                self.status,
                if self.body.is_empty() { "no" } else { "some" },
                self.media_type()
            )
        } else {
            self.plain()
        }
    }

    /// A body that is not the asset: its media type, and its text where it
    /// is text, with every id taken out.
    fn something_else(&self) -> String {
        let media_type = self.media_type();
        if String::from_utf8_lossy(&self.body).contains("Content-Range: bytes") {
            return format!("{media_type}, a multipart body");
        }
        match std::str::from_utf8(&self.body) {
            Ok(text) => {
                let text = without_tags(text);
                let text: Vec<&str> = text.split_whitespace().collect();
                let text: String = without_ids(&text.join(" ")).chars().take(80).collect();
                format!("{media_type} \"{text}\"")
            }
            Err(_) => format!("{media_type}, {} bytes", self.body.len()),
        }
    }
}

/// `text` with every `<…>` tag replaced by a space: an error page's words.
pub(crate) fn without_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for character in text.chars() {
        match character {
            '<' => {
                inside = true;
                out.push(' ');
            }
            '>' if inside => inside = false,
            _ if inside => {}
            other => out.push(other),
        }
    }
    out
}

/// An offset as a metric writes it: against the asset's length `N` where it
/// is within ten bytes of the end or past it, a number otherwise.
fn offset(at: usize, total: usize) -> String {
    if at == total {
        "N".to_owned()
    } else if at > total {
        format!("N+{}", at.saturating_sub(total))
    } else if total.saturating_sub(at) <= 10 {
        format!("N-{}", total.saturating_sub(at))
    } else {
        at.to_string()
    }
}

/// A span of bytes, first and last.
type Span = (usize, usize);

/// Reads a `Content-Range` of bytes: the span, where one is stated, and the
/// total, where one is.
fn parse_content_range(value: &str) -> Option<(Option<Span>, Option<usize>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        number => Some(number.parse().ok()?),
    };
    let span = match span.trim() {
        "*" => None,
        span => {
            let (first, last) = span.split_once('-')?;
            Some((first.trim().parse().ok()?, last.trim().parse().ok()?))
        }
    };
    Some((span, total))
}

/// `text` with every UUID replaced by `<id>` and every other run of four or
/// more hexadecimal digits that has a digit in it by `<n>`: an error names
/// the asset asked for, and a proxy's names the request.
pub(crate) fn without_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        if let Some(candidate) = rest.get(..36)
            && Uuid::parse_str(candidate).is_ok()
            && candidate.contains('-')
        {
            out.push_str("<id>");
            rest = rest.get(36..).unwrap_or_default();
            continue;
        }
        let run: usize = rest
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .map(char::len_utf8)
            .sum();
        let word = rest.get(..run).unwrap_or_default();
        if run >= 4 && word.chars().any(|digit| digit.is_ascii_digit()) {
            out.push_str("<n>");
            rest = rest.get(run..).unwrap_or_default();
        } else if run > 0 {
            out.push_str(word);
            rest = rest.get(run..).unwrap_or_default();
        } else {
            out.push(first);
            rest = rest.get(first.len_utf8()..).unwrap_or_default();
        }
    }
    out
}

/// The scheme, host and port of a URL, as one text.
fn authority_of(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    Some(format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str()?,
        url.port_or_known_default()?
    ))
}

/// The client the census asks with, and what it met on the way.
#[derive(Debug)]
struct Asker {
    /// An HTTP client that follows no redirect.
    http: reqwest::Client,
    /// How many answers were a `502`, `503` or `504`.
    transient: u32,
    /// The first such answer.
    first_transient: String,
    /// How many answers were a redirect.
    redirects: u32,
}

impl Asker {
    /// A client that follows no redirect, so that one is an answer.
    fn new() -> Result<Self, TestFailure> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                TestFailure::Assertion(format!("building an HTTP client failed: {error}"))
            })?;
        Ok(Self {
            http,
            transient: 0,
            first_transient: "none".to_owned(),
            redirects: 0,
        })
    }

    /// Sends one request and reads the answer, asking again while the answer
    /// is a transient status — unless the request is for something the grid
    /// has not got (`again` false), where that status may be the answer.
    async fn ask(
        &mut self,
        method: &Method,
        url: &str,
        headers: &[(&str, &str)],
        again: bool,
    ) -> Result<Reply, TestFailure> {
        let mut attempt = 0_u32;
        loop {
            let mut request = self.http.request(method.clone(), url);
            if *method == Method::POST {
                // An empty body with its length stated: the question is the
                // method, not a request with no length.
                request = request.body("");
            }
            for (name, value) in headers {
                let name = ReqwestHeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    TestFailure::Assertion(format!("{name} is no header name: {error}"))
                })?;
                let value = ReqwestHeaderValue::from_str(value).map_err(|error| {
                    TestFailure::Assertion(format!("{value} is no header value: {error}"))
                })?;
                request = request.header(name, value);
            }
            let response = request.send().await.map_err(|error| {
                TestFailure::Assertion(format!("an asset request failed: {error}"))
            })?;
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let body = response
                .bytes()
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("reading an asset answer failed: {error}"))
                })?
                .to_vec();
            let reply = Reply {
                status,
                headers,
                body,
            };
            if (300..400).contains(&status) && status != 304 {
                self.redirects = self.redirects.saturating_add(1);
            }
            if again && matches!(status, 502..=504) {
                if self.transient == 0 {
                    self.first_transient = format!(
                        "{}; retry-after {}",
                        reply.plain(),
                        reply.header("retry-after").unwrap_or("not stated")
                    );
                }
                self.transient = self.transient.saturating_add(1);
                if attempt < TRANSIENT_RETRIES {
                    attempt = attempt.saturating_add(1);
                    tokio::time::sleep(TRANSIENT_PAUSE).await;
                    continue;
                }
            }
            return Ok(reply);
        }
    }

    /// GETs `url` with `headers`, where the grid has what is asked for.
    async fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, TestFailure> {
        self.ask(&Method::GET, url, headers, true).await
    }

    /// GETs `url` with `headers` once, where the grid may have nothing to
    /// answer with.
    async fn get_once(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<Reply, TestFailure> {
        self.ask(&Method::GET, url, headers, false).await
    }
}

/// The headers of an answer that tell a cache or a fetcher what it may do, as
/// a metric records them: the value of those that say how, the presence of
/// those whose value is the asset's own.
fn cache_headers(reply: &Reply) -> String {
    let mut stated = Vec::new();
    for name in [
        "accept-ranges",
        "cache-control",
        "content-disposition",
        "pragma",
        "vary",
    ] {
        if let Some(value) = reply.header(name) {
            stated.push(format!("{name}: {}", without_ids(value)));
        }
    }
    for name in ["etag", "expires", "last-modified"] {
        if reply.headers.contains_key(name) {
            stated.push(name.to_owned());
        }
    }
    if stated.is_empty() {
        "none".to_owned()
    } else {
        stated.join("; ")
    }
}

/// Asks one capability every question and records the answers. `false` where
/// it had no asset to ask about, or the capability answered none of them.
async fn census(
    asker: &mut Asker,
    target: &Target,
    simulator: Option<&str>,
    metrics: &mut Metrics,
) -> Result<bool, TestFailure> {
    let prefix = target.prefix;
    let accept = ("accept", target.accept);
    let Some(cap) = &target.cap else {
        for question in QUESTIONS {
            metrics.set(&format!("{prefix}_{question}"), NO_CAPABILITY);
        }
        return Ok(true);
    };
    // The shape of the capability's URL — the bare root of a host, or a path
    // with a token of the capability's own — and, apart, the host it names.
    let parsed = url::Url::parse(cap).ok();
    metrics.set(
        &format!("{prefix}_url_shape"),
        match &parsed {
            None => "no URL",
            Some(url) if url.query().is_some() => "a query",
            Some(url) if url.path() == "/" => "the root of its host",
            Some(url) if without_ids(url.path()).contains("<id>") => "a path with a token",
            Some(_) => "a path",
        },
    );
    metrics.set(
        &format!("{prefix}_url_host"),
        parsed
            .as_ref()
            .and_then(url::Url::host_str)
            .unwrap_or("no host")
            .to_owned(),
    );
    // The whole asset: of the first candidate the capability answers.
    let mut found = None;
    let mut refused = 0_i64;
    let mut last_refusal = None;
    for (key, asset) in &target.assets {
        let whole = asker
            .get_once(&format!("{cap}/?{key}={asset}"), &[accept])
            .await?;
        if whole.status == 200 && whole.body.len() >= SHORTEST_ASSET {
            found = Some((*key, *asset, whole));
            break;
        }
        refused = refused.saturating_add(1);
        last_refusal = Some(whole.plain());
    }
    metrics.set(&format!("{prefix}_assets_not_answered"), refused);
    metrics.set(
        &format!("{prefix}_asset_not_answered_with"),
        last_refusal.clone().unwrap_or_else(|| "none".to_owned()),
    );
    let Some((key, asset, whole)) = found else {
        let stand_in = if target.assets.is_empty() {
            NO_ASSET
        } else {
            NO_ANSWER
        };
        for question in QUESTIONS {
            metrics.set(&format!("{prefix}_{question}"), stand_in);
        }
        if let Some(refusal) = last_refusal {
            metrics.set(&format!("{prefix}_whole"), refusal);
        }
        return Ok(false);
    };
    metrics.set(&format!("{prefix}_asked_under"), key);
    let mut note = |question: &str, answer: String| {
        metrics.set(&format!("{prefix}_{question}"), answer);
    };
    let url = format!("{cap}/?{key}={asset}");

    let authority = authority_of(cap);
    note(
        "url",
        format!(
            "{}, {}",
            cap.split_once(':')
                .map_or("no scheme", |(scheme, _rest)| scheme),
            if authority.is_some() && authority.as_deref() == simulator {
                "the simulator's host and port"
            } else {
                "another host or port"
            }
        ),
    );

    let bytes = whole.body.clone();
    let total = bytes.len();
    note(
        "whole",
        format!(
            "{}: {} as {}",
            whole.status,
            if bytes.is_empty() {
                "no body"
            } else {
                "the asset"
            },
            whole.media_type()
        ),
    );
    note("whole_headers", cache_headers(&whole));
    note(
        "whole_encoding",
        whole
            .header("content-encoding")
            .unwrap_or("not stated")
            .to_owned(),
    );
    let mut names: Vec<&str> = whole
        .headers
        .keys()
        .map(ReqwestHeaderName::as_str)
        .collect();
    names.sort_unstable();
    names.dedup();
    note("whole_every_header", names.join(", "));

    // Ranges.
    let last = total.saturating_sub(1);
    let ranges = [
        ("range_first", "bytes=0-9".to_owned()),
        ("range_first_byte", "bytes=0-0".to_owned()),
        ("range_middle", "bytes=10-19".to_owned()),
        ("range_open", "bytes=10-".to_owned()),
        (
            "range_end_past",
            format!("bytes=10-{}", total.saturating_add(99)),
        ),
        ("range_whole", format!("bytes=0-{last}")),
        (
            "range_wider",
            format!("bytes=0-{}", total.saturating_add(99)),
        ),
        (
            "range_start_at_end",
            format!("bytes={total}-{}", total.saturating_add(9)),
        ),
        (
            "range_start_past",
            format!(
                "bytes={}-{}",
                total.saturating_add(10),
                total.saturating_add(19)
            ),
        ),
        ("range_open_at_end", format!("bytes={total}-")),
        ("range_suffix", "bytes=-10".to_owned()),
        ("range_two", "bytes=0-9,20-29".to_owned()),
        ("range_backwards", "bytes=19-10".to_owned()),
        ("range_no_number", "bytes=x-y".to_owned()),
        ("range_other_unit", "items=0-9".to_owned()),
    ];
    for (question, range) in &ranges {
        let reply = asker.get(&url, &[accept, ("range", range)]).await?;
        let mut answer = reply.against(&bytes);
        if reply.media_type().starts_with("multipart/") {
            answer = format!("{}: {}", reply.status, reply.media_type());
        }
        note(question, answer);
    }

    // Conditions.
    let conditions = [
        ("if_none_match", "if-none-match", "etag", "no etag stated"),
        (
            "if_modified_since",
            "if-modified-since",
            "last-modified",
            "no last-modified stated",
        ),
    ];
    for (question, condition, stated, absent) in conditions {
        let answer = match whole.header(stated) {
            Some(value) => asker
                .get(&url, &[accept, (condition, value)])
                .await?
                .against(&bytes),
            None => absent.to_owned(),
        };
        note(question, answer);
    }

    // Other requests.
    let head = asker.ask(&Method::HEAD, &url, &[accept], true).await?;
    let length = head
        .header("content-length")
        .and_then(|value| value.parse::<usize>().ok());
    note(
        "head",
        format!(
            "{}: content-length {}",
            head.status,
            match length {
                Some(length) if length == total => "N".to_owned(),
                Some(length) => length.to_string(),
                None => "not stated".to_owned(),
            }
        ),
    );
    let post = asker.ask(&Method::POST, &url, &[accept], false).await?;
    note("post", post.against(&bytes));
    let other = asker.get(&url, &[("accept", "text/html")]).await?;
    note(
        "other_accept",
        format!(
            "{}: {} as {}",
            other.status,
            if other.body == bytes {
                "the asset"
            } else if other.body.is_empty() {
                "no body"
            } else {
                "something else"
            },
            other.media_type()
        ),
    );
    let gzip = asker
        .get(&url, &[accept, ("accept-encoding", "gzip")])
        .await?;
    note(
        "gzip",
        format!(
            "{}: {}",
            gzip.status,
            match gzip.header("content-encoding") {
                how if how == whole.header("content-encoding") => "as without it".to_owned(),
                Some(how) => format!("encoded {how}"),
                None => "not encoded".to_owned(),
            }
        ),
    );
    let no_slash = asker
        .get(&format!("{cap}?{key}={asset}"), &[accept])
        .await?;
    note("no_slash", no_slash.against(&bytes));

    // What is not there.
    let missing = format!("{cap}/?{key}={NO_SUCH_ASSET}");
    note(
        "missing",
        asker.get_once(&missing, &[accept]).await?.plain(),
    );
    note(
        "missing_range",
        asker
            .get_once(&missing, &[accept, ("range", "bytes=0-9")])
            .await?
            .plain(),
    );
    let nil = format!("{cap}/?{key}={}", Uuid::nil());
    note("nil_id", asker.get_once(&nil, &[accept]).await?.plain());
    let no_id = format!("{cap}/?{key}=plywood");
    note("no_id", asker.get_once(&no_id, &[accept]).await?.plain());
    note(
        "no_query",
        asker.get_once(&format!("{cap}/"), &[accept]).await?.plain(),
    );
    let other_key = format!("{cap}/?asset_id={asset}");
    note(
        "other_key",
        asker.get_once(&other_key, &[accept]).await?.plain(),
    );
    let other_class = match target.other_class {
        Some(other) if target.prefix == "asset" => {
            // `ViewerAsset` is asked for the asset it has under the key of
            // another class.
            let _other = other;
            let another = if key == "sound_id" {
                "bodypart_id"
            } else {
                "sound_id"
            };
            asker
                .get_once(&format!("{cap}/?{another}={asset}"), &[accept])
                .await?
                .against(&bytes)
        }
        Some(other) => asker
            .get_once(&format!("{cap}/?{key}={other}"), &[accept])
            .await?
            .plain_or_bytes(),
        None => NO_ASSET.to_owned(),
    };
    // What media type an asset of another class comes as is recorded apart:
    // the fake grid's store does not keep an asset's class.
    let (other_class, other_class_as) = match other_class.split_once(" as ") {
        Some((answer, media_type)) => (answer.to_owned(), media_type.to_owned()),
        None => (other_class, "nothing".to_owned()),
    };
    note("other_class_as", other_class_as);
    note("other_class", other_class);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{offset, parse_content_range, without_ids};

    /// An offset near or past the end is written against the length.
    #[test]
    fn offsets_are_written_against_the_end() {
        assert_eq!(offset(0, 600), "0");
        assert_eq!(offset(19, 600), "19");
        assert_eq!(offset(589, 600), "589");
        assert_eq!(offset(590, 600), "N-10");
        assert_eq!(offset(599, 600), "N-1");
        assert_eq!(offset(600, 600), "N");
        assert_eq!(offset(699, 600), "N+99");
    }

    /// A content range is read with or without a span and a total.
    #[test]
    fn content_ranges_are_read() {
        assert_eq!(
            parse_content_range("bytes 0-9/600"),
            Some((Some((0, 9)), Some(600)))
        );
        assert_eq!(parse_content_range("bytes */600"), Some((None, Some(600))));
        assert_eq!(
            parse_content_range("bytes 0-9/*"),
            Some((Some((0, 9)), None))
        );
        assert_eq!(parse_content_range("items 0-9/600"), None);
        assert_eq!(parse_content_range("bytes 0-9"), None);
    }

    /// An id and a request number are taken out of an error's text; a word
    /// that happens to be hexadecimal is not.
    #[test]
    fn ids_are_taken_out() {
        assert_eq!(
            without_ids("asset 00000bad-a55e-7000-8000-00000000dead not found"),
            "asset <id> not found"
        );
        assert_eq!(
            without_ids("Reference #18.4c1d3e17.1760000000.abc12345 bad feed"),
            "Reference #18.<n>.<n>.<n> bad feed"
        );
    }
}
