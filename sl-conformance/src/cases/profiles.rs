//! A census of profiles: how each grid reads an avatar's profile, over UDP and
//! over the `AgentProfile` capability; what an edit of one's own profile,
//! interests and notes takes over each road; what picks and classifieds bring
//! and cost; how display names are read; and what the profile-image upload
//! answers.
//!
//! Avatar **A** reads and writes; avatar **B** is the other avatar it reads
//! and keeps notes about. One question a leg:
//!
//! 1. **Reading another's profile**: A asks for B's profile over UDP and over
//!    the capability; what each brings A, and whether B is told.
//! 2. **Reading one's own** the same two ways.
//! 3. **Editing over UDP**: A sends `AvatarPropertiesUpdate`; what each road
//!    reads back.
//! 4. **Editing over the capability**: a one-key `PUT`, a two-key `PUT`, the
//!    `hide_age` flag and a text longer than the UDP field holds.
//! 5. **Interests** over UDP.
//! 6. **Notes** about B: asked for over UDP, written over UDP and over the
//!    capability, read back both ways.
//! 7. **Picks**: both lists asked for, one created, read, edited and deleted.
//! 8. **Classifieds**: both lists asked for, one listed for nothing and one for
//!    L$ 50, read, its click counts asked for over `SearchStatRequest`, edited
//!    and deleted.
//! 9. **The profile image** over `UploadAgentProfileImage`, put back after.
//! 10. **The client's own commands** — a read of B's profile, an edit, a note,
//!     a classified's click counts and an image upload — which take the
//!     capability where the region grants it.
//!
//! Display names are the `display-names` census's.
//!
//! Every "UDP" leg sends its message raw, as the reference packs it, so that
//! it measures the UDP road on a grid where the client itself would take the
//! capability.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/profiles.md`; every run is held to it.
//!
//! `2av`, live grids only: the fake grid serves no profiles yet
//! (`server-fake-grid-profiles`). A's profile, interests and notes about B are
//! put back as they were found, and every pick and classified the census made
//! is deleted, also when a leg fails.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AnyMessage, AvatarClassified, AvatarInterests, AvatarPick, AvatarProperties,
    ClassifiedCategory, ClassifiedClickStats, ClassifiedInfo, ClassifiedKey, ClassifiedUpdate,
    Command, Event, InstantMessage, InterestsUpdate, LindenAmount, MoneyBalance, PickInfo, PickKey,
    PickUpdate, ProfileImage, ProfileUpdate, Reliability, TextureKey, Throttle, Uuid,
};
use sl_wire::messages::{
    AvatarNotesUpdate, AvatarNotesUpdateAgentDataBlock, AvatarNotesUpdateDataBlock,
    AvatarPropertiesRequest, AvatarPropertiesRequestAgentDataBlock, AvatarPropertiesUpdate,
    AvatarPropertiesUpdateAgentDataBlock, AvatarPropertiesUpdatePropertiesDataBlock,
    GenericMessage, GenericMessageAgentDataBlock, GenericMessageMethodDataBlock,
    GenericMessageParamListBlock,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::without_ids;
use crate::cases::groups::{names_of, without_dates};
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{hold_to_measured, http_client, shape_of};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, secs_metric};

/// What every text of the census begins with.
const TEXT_TAG: &str = "pf-";

/// How long an avatar listens for the answer to what it asked, at most.
const LISTEN: Duration = Duration::from_secs(10);

/// How long an avatar listens on after the answer it came for, for what else
/// the grid sends.
const QUIET: Duration = Duration::from_secs(3);

/// How long the other avatar is listened to after a leg, for what it was told
/// meanwhile.
const DRAIN: Duration = Duration::from_secs(1);

/// How long a grid is given to take a write before it is read back.
const SETTLE: Duration = Duration::from_secs(3);

/// How long the client's own profile image upload is waited for: it tries
/// three times, as the reference does.
const CLIENT_UPLOAD: Duration = Duration::from_secs(60);

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// The capability that reads and writes profiles.
const AGENT_PROFILE: &str = "AgentProfile";

/// The capability that uploads a profile image.
const UPLOAD_AGENT_PROFILE_IMAGE: &str = "UploadAgentProfileImage";

/// The capability that reads a classified's click counts.
const SEARCH_STAT_REQUEST: &str = "SearchStatRequest";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// What Second Life charges at least to list a classified (the reference's
/// `MINIMUM_PRICE_FOR_LISTING`).
const LISTING_PRICE: u64 = 50;

/// The id the census lists its free classified under on OpenSim.
const OPENSIM_FREE_CLASSIFIED: Uuid = Uuid::from_u128(0x5c11_0029_c1a5_51f1_ed00_0000_0000_0011);

/// The id the census lists its paid classified under on OpenSim.
const OPENSIM_PAID_CLASSIFIED: Uuid = Uuid::from_u128(0x5c11_0029_c1a5_51f1_ed00_0000_0000_0012);

/// How long the census's long about text is: longer than the 512 bytes the
/// UDP field holds.
const LONG_TEXT_LENGTH: usize = 1000;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/profiles.md (profiles, 2026-10-15)";

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
/// under: every run is held to it.
///
/// Not held: how long the census took.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "classified_edit_read_told_a",
        each(
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified edited description text, parcel some, estate 1, snapshot none, region none, position zero, parcel name none, flags 0x0, price 50, created a time, expires zero | click counts of the paid classified: teleport 0, map 0, profile 0, none from search",
            ),
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified edited description text, parcel some, estate 0, snapshot none, region some, position zero, parcel name some, flags 0x4, price 50, created a time, expires after the creation",
            ),
        ),
    ),
    (
        "classified_edit_told_a",
        each(
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified edited description text, parcel some, estate 1, snapshot none, region none, position zero, parcel name none, flags 0x0, price 50, created a time, expires zero | click counts of the paid classified: teleport 0, map 0, profile 0, none from search",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("classified_free_charged", both(Answer::Count(0))),
    (
        "classified_free_delete_read_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "classified the free classified by avatar A: category Shopping, name the free classified text, description the classified description text, parcel some, estate 0, snapshot none, region some, position zero, parcel name some, flags 0x4, price 0, created a time, expires after the creation",
            ),
        ),
    ),
    (
        "classified_free_delete_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert \"/Error deleting classified\""),
        ),
    ),
    (
        "classified_free_read_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "classified the free classified by avatar A: category Shopping, name the free classified text, description the classified description text, parcel some, estate 0, snapshot none, region some, position zero, parcel name some, flags 0x4, price 0, created a time, expires after the creation",
            ),
        ),
    ),
    (
        "classified_free_told_a",
        each(
            Answer::Text("alert \"Invalid price for listing, classified not published.\""),
            Answer::Text("nothing"),
        ),
    ),
    (
        "classified_paid_charged",
        each(Answer::Count(50), Answer::Count(0)),
    ),
    (
        "classified_paid_delete_read_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified edited description text, parcel some, estate 0, snapshot none, region some, position zero, parcel name some, flags 0x4, price 50, created a time, expires after the creation",
            ),
        ),
    ),
    (
        "classified_paid_delete_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert \"/Error deleting classified\""),
        ),
    ),
    (
        "classified_paid_read_told_a",
        each(
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified description text, parcel some, estate 1, snapshot none, region none, position zero, parcel name none, flags 0x0, price 50, created a time, expires zero | click counts of the paid classified: teleport 0, map 0, profile 0, none from search",
            ),
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified description text, parcel some, estate 0, snapshot none, region some, position zero, parcel name some, flags 0x4, price 50, created a time, expires after the creation",
            ),
        ),
    ),
    (
        "classified_paid_told_a",
        each(
            Answer::Text(
                "classified the paid classified by avatar A: category Shopping, name the paid classified text, description the classified description text, parcel some, estate 1, snapshot none, region none, position zero, parcel name none, flags 0x0, price 50, created a time, expires zero | balance: type 1103, amount 50, \"You paid Commerce Linden L$50.\" | balance: type 1103, amount 50, \"You paid Commerce Linden L$50.\" | click counts of the paid classified: teleport 0, map 0, profile 0, none from search",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "classified_stats_answer",
        each(
            Answer::Text(
                "200, {map_clicks=0, profile_clicks=0, search_map_clicks=0, search_profile_clicks=0, search_teleport_clicks=0, teleport_clicks=0}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "classifieds_listed_told_a",
        each(
            Answer::Text("classifieds of avatar A: [the paid classified the paid classified text]"),
            Answer::Text(
                "classifieds of avatar A: [the free classified the free classified text, the paid classified the paid classified text, some of the avatar's own]",
            ),
        ),
    ),
    (
        "classifieds_other_ask_told_a",
        both(Answer::Text("classifieds of avatar B: []")),
    ),
    (
        "classifieds_other_reads_told_b",
        each(
            Answer::Text("classifieds of avatar A: [the paid classified the paid classified text]"),
            Answer::Text(
                "classifieds of avatar A: [the free classified the free classified text, the paid classified the paid classified text, some of the avatar's own]",
            ),
        ),
    ),
    (
        "classifieds_own_ask_told_a",
        each(
            Answer::Text("classifieds of avatar A: []"),
            Answer::Text("classifieds of avatar A: [some of the avatar's own]"),
        ),
    ),
    (
        "client_click_stats_told_a",
        each(
            Answer::Text(
                "click counts of the paid classified: teleport 0, map 0, profile 0, from search teleport 0, map 0, profile 0",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "client_edit_read_told_a",
        each(
            Answer::Text(
                "profile avatar A: about the client about text, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x1 the want to text, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar A: about the client about text, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x1 the want to text, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "client_edit_reads_cap",
        each(
            Answer::Text("the client about text"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "client_edit_reads_udp",
        both(Answer::Text("the client about text")),
    ),
    (
        "client_edit_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "profile avatar A: about the client about text, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x1 the want to text, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "client_image_reads_cap",
        each(
            Answer::Text("the client's uploaded image"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "client_image_told_a",
        each(
            Answer::Text("FirstLife image uploaded: the client's uploaded image"),
            Answer::Text("FirstLife image uploaded: no asset"),
        ),
    ),
    (
        "client_notes_reads",
        both(Answer::Text("the client note text")),
    ),
    ("client_notes_told_a", both(Answer::Text("nothing"))),
    (
        "client_read_other_told_a",
        each(
            Answer::Text(
                "profile avatar B: about empty, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x1 | groups of avatar B: none, shown in the profile true | picks of avatar B: [] | notes about avatar B: the cap note text",
            ),
            Answer::Text(
                "profile avatar B: about empty, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar B: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "edit_udp_read_told_a",
        each(
            Answer::Text(
                "profile avatar A: about the udp about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar A: about the udp about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "edit_udp_reads_cap",
        each(
            Answer::Text(
                "about the udp about text, first life about the udp first life about text",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "edit_udp_reads_udp",
        both(Answer::Text(
            "about the udp about text, first life about the udp first life about text",
        )),
    ),
    (
        "edit_udp_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "profile avatar A: about the udp about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    ("edit_udp_told_b", both(Answer::Text("nothing"))),
    (
        "image_read_told_a",
        each(
            Answer::Text(
                "profile avatar A: about the long about text cut to 500 bytes, first life about the cap two first life about text, image nobody, first life image the uploaded image, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x1 the want to text, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "image_reads_cap",
        each(
            Answer::Text("the uploaded image"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "image_reads_udp",
        each(
            Answer::Text("the uploaded image"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "image_told_a",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "image_upload_answer",
        each(
            Answer::Text("200, {new_asset=the uploaded image, state=\"complete\"}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "image_uploader_answer",
        each(
            Answer::Text("200, {state=\"upload\", uploader=a URL}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "interests_read_told_a",
        each(
            Answer::Text(
                "profile avatar A: about the long about text cut to 500 bytes, first life about the cap two first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x1 the want to text, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar A: about the udp about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x1 the want to text, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "interests_reads",
        both(Answer::Text("want to the want to text, mask as sent")),
    ),
    ("interests_told_a", both(Answer::Text("nothing"))),
    ("login_web_profile_url", both(Answer::Text("none"))),
    (
        "notes_ask_told_a",
        both(Answer::Text("notes about avatar B: empty")),
    ),
    (
        "notes_cap_answer",
        each(Answer::Text("200, undef"), Answer::Text("no capability")),
    ),
    (
        "notes_cap_reads_cap",
        each(
            Answer::Text("the cap note text"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "notes_cap_reads_udp",
        each(
            Answer::Text("the cap note text"),
            Answer::Text("the udp note text"),
        ),
    ),
    ("notes_cap_told_b", both(Answer::Text("nothing"))),
    (
        "notes_udp_reads_cap",
        each(
            Answer::Text("the udp note text"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "notes_udp_reads_udp",
        both(Answer::Text("the udp note text")),
    ),
    ("notes_udp_told_a", both(Answer::Text("nothing"))),
    ("notes_udp_told_b", both(Answer::Text("nothing"))),
    (
        "pick_create_told_a",
        each(
            Answer::Text(
                "picks of avatar A: [the pick the pick text] | pick the pick by avatar A: top false, parcel some, name the pick text, description the pick description text, snapshot none, user the name \"Governor Linden\", parcel name none, region none, position zero, sort 0, enabled true",
            ),
            Answer::Text(
                "picks of avatar A: [the pick the pick text] | pick the pick by avatar A: top false, parcel some, name the pick text, description the pick description text, snapshot none, user the name \"Your Parcel\", parcel name none, region some, position set, sort 0, enabled true",
            ),
        ),
    ),
    ("pick_create_told_b", both(Answer::Text("nothing"))),
    (
        "pick_delete_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("picks of avatar A: []"),
        ),
    ),
    (
        "pick_deleted_cap",
        each(Answer::Text("[]"), Answer::Text("no capability")),
    ),
    (
        "pick_deleted_list_told_a",
        both(Answer::Text("picks of avatar A: []")),
    ),
    (
        "pick_edit_told_a",
        each(
            Answer::Text(
                "picks of avatar A: [the pick the pick text] | pick the pick by avatar A: top false, parcel some, name the pick text, description the pick edited description text, snapshot none, user the name \"Governor Linden\", parcel name none, region none, position zero, sort 0, enabled true",
            ),
            Answer::Text(
                "picks of avatar A: [the pick the pick text] | pick the pick by avatar A: top false, parcel some, name the pick text, description the pick edited description text, snapshot none, user the name \"Your Parcel\", parcel name none, region some, position set, sort 0, enabled true",
            ),
        ),
    ),
    (
        "pick_listed_cap",
        each(
            Answer::Text(
                "[{description=the pick description text, enabled=true, grid_x=0, grid_y=0, id=the pick, name=the pick text, parcel_id=another id, parcel_name=\"\", region_name=\"\", region_x=0, region_y=0, region_z=0, slurl=a URL, snapshot_id=nobody}]",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "pick_other_reads_info_told_b",
        each(
            Answer::Text(
                "pick the pick by avatar A: top false, parcel some, name the pick text, description the pick edited description text, snapshot none, user the name \"Governor Linden\", parcel name none, region none, position zero, sort 0, enabled true",
            ),
            Answer::Text(
                "pick the pick by avatar A: top false, parcel some, name the pick text, description the pick edited description text, snapshot none, user the name \"Your Parcel\", parcel name none, region some, position set, sort 0, enabled true",
            ),
        ),
    ),
    (
        "pick_other_reads_told_b",
        both(Answer::Text("picks of avatar A: [the pick the pick text]")),
    ),
    (
        "pick_read_told_a",
        each(
            Answer::Text(
                "pick the pick by avatar A: top false, parcel some, name the pick text, description the pick description text, snapshot none, user the name \"Governor Linden\", parcel name none, region none, position zero, sort 0, enabled true",
            ),
            Answer::Text(
                "pick the pick by avatar A: top false, parcel some, name the pick text, description the pick description text, snapshot none, user the name \"Your Parcel\", parcel name none, region some, position set, sort 0, enabled true",
            ),
        ),
    ),
    (
        "picks_other_ask_told_a",
        both(Answer::Text("picks of avatar B: []")),
    ),
    (
        "picks_own_ask_told_a",
        both(Answer::Text("picks of avatar A: []")),
    ),
    (
        "put_hide_age_answer",
        each(Answer::Text("200, undef"), Answer::Text("no capability")),
    ),
    (
        "put_hide_age_reads",
        each(
            Answer::Text("absent, not as put"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "put_hide_age_seen_by_b",
        each(
            Answer::Text("member since a date, hide age absent"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "put_long_answer",
        each(Answer::Text("200, undef"), Answer::Text("no capability")),
    ),
    (
        "put_long_read_told_a",
        each(
            Answer::Text(
                "profile avatar A: about the long about text cut to 500 bytes, first life about the cap two first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar A: about the udp about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "put_long_reads_cap",
        each(
            Answer::Text("the long about text"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "put_long_reads_udp",
        each(
            Answer::Text("the long about text cut to 500 bytes"),
            Answer::Text("the udp about text"),
        ),
    ),
    (
        "put_one_answer",
        each(Answer::Text("200, undef"), Answer::Text("no capability")),
    ),
    (
        "put_one_read_told_a",
        each(
            Answer::Text(
                "profile avatar A: about the cap about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar A: about the udp about text, first life about the udp first life about text, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    (
        "put_one_reads_cap",
        each(
            Answer::Text("the cap about text"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "put_one_reads_udp",
        each(
            Answer::Text("the cap about text"),
            Answer::Text("the udp about text"),
        ),
    ),
    (
        "put_two_answer",
        each(Answer::Text("200, undef"), Answer::Text("no capability")),
    ),
    (
        "put_two_reads_cap",
        each(
            Answer::Text(
                "about the cap two about text, first life about the cap two first life about text",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "read_other_cap_answer",
        each(
            Answer::Text(
                "200, {allow_publish=true, allowed_maturity=42, charter_member=1, customer_type=\"Base\", display_name=\"<B>\", display_name_next_update=a date, fl_about_text=undef, fl_image_id=nobody, groups=[], home_page=undef, id=avatar B, identified=false, is_display_name_default=true, legacy_first_name=\"<B>\", legacy_last_name=\"Resident\", mature_profile=false, member_since=a date, notes=empty, partner_id=nobody as a string, picks=[], preferred_maturity=0, sl_about_text=undef, sl_image_id=nobody, title=undef, transacted=false, username=\"<B>\"}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "read_other_cap_shape",
        each(
            Answer::Text(
                "{allow_publish:boolean,allowed_maturity:integer,charter_member:integer,customer_type:string,display_name:string,display_name_next_update:date,fl_about_text:undef,fl_image_id:uuid,groups:[0],home_page:undef,id:uuid,identified:boolean,is_display_name_default:boolean,legacy_first_name:string,legacy_last_name:string,mature_profile:boolean,member_since:date,notes:string,partner_id:string,picks:[0],preferred_maturity:integer,sl_about_text:undef,sl_image_id:uuid,title:undef,transacted:boolean,username:string}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("read_other_cap_told_b", both(Answer::Text("nothing"))),
    (
        "read_other_udp_told_a",
        each(
            Answer::Text(
                "profile avatar B: about empty, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar B: want to 0x0 empty, skills 0x0 empty, languages empty | groups of avatar B: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar B: about empty, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar B: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
    ("read_other_udp_told_b", both(Answer::Text("nothing"))),
    (
        "read_own_cap_answer",
        each(
            Answer::Text(
                "200, {allow_publish=true, allowed_maturity=42, charter_member=1, customer_type=\"Base\", display_name=\"<A>\", display_name_next_update=a date, fl_about_text=empty, fl_image_id=nobody, groups=[], home_page=\"\", id=avatar A, identified=false, is_display_name_default=true, legacy_first_name=\"<A>\", legacy_last_name=\"Resident\", mature_profile=false, member_since=a date, notes=undef, online=true, partner_id=nobody as a string, picks=[], preferred_maturity=42, sl_about_text=empty, sl_image_id=nobody, title=undef, transacted=false, username=\"<A>\"}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "read_own_cap_shape",
        each(
            Answer::Text(
                "{allow_publish:boolean,allowed_maturity:integer,charter_member:integer,customer_type:string,display_name:string,display_name_next_update:date,fl_about_text:string,fl_image_id:uuid,groups:[0],home_page:string,id:uuid,identified:boolean,is_display_name_default:boolean,legacy_first_name:string,legacy_last_name:string,mature_profile:boolean,member_since:date,notes:undef,online:boolean,partner_id:string,picks:[0],preferred_maturity:integer,sl_about_text:string,sl_image_id:uuid,title:undef,transacted:boolean,username:string}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "read_own_udp_told_a",
        each(
            Answer::Text(
                "profile avatar A: about empty, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\\u{1}\", flags 0x11 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty | groups of avatar A: some, shown in the profile true",
            ),
            Answer::Text(
                "profile avatar A: about 23 bytes of the avatar's own, first life about empty, image nobody, first life image nobody, partner nobody, born d/dd/dddd, url \"\", charter \"\", flags 0x10 | interests avatar A: want to 0x0 empty, skills 0x0 empty, languages empty",
            ),
        ),
    ),
];

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every profile read.
    properties: Vec<AvatarProperties>,
    /// Every interests read.
    interests: Vec<AvatarInterests>,
    /// Every profile group list: whose, how many, whether listed.
    groups: Vec<(Uuid, usize, bool)>,
    /// Every picks list, by whose.
    picks: Vec<(Uuid, Vec<AvatarPick>)>,
    /// Every notes read, by whom about.
    notes: Vec<(Uuid, String)>,
    /// Every classifieds list, by whose.
    classifieds: Vec<(Uuid, Vec<AvatarClassified>)>,
    /// Every pick read.
    pick_infos: Vec<PickInfo>,
    /// Every classified read.
    classified_infos: Vec<ClassifiedInfo>,
    /// The method of every generic message.
    generic: Vec<String>,
    /// Every alert.
    alerts: Vec<String>,
    /// Every balance.
    money: Vec<MoneyBalance>,
    /// Every instant message.
    ims: Vec<InstantMessage>,
    /// How many display-name changes were pushed.
    display_name_updates: usize,
    /// How many display-name set answers were pushed.
    set_display_name_replies: usize,
    /// Every classified's click counts.
    click_stats: Vec<ClassifiedClickStats>,
    /// Every profile image upload answered: which image, and its asset.
    images: Vec<(ProfileImage, Option<TextureKey>)>,
}

impl Got {
    /// Whether the window brought a profile of `who`.
    fn has_properties(&self, who: Uuid) -> bool {
        self.properties
            .iter()
            .any(|props| props.avatar_id.uuid() == who)
    }
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event: &Event) -> Option<()> {
    match event {
        Event::AvatarProperties(props) => got.properties.push((**props).clone()),
        Event::AvatarInterests(interests) => got.interests.push((**interests).clone()),
        Event::AvatarGroups {
            avatar_id,
            groups,
            list_in_profile,
        } => got
            .groups
            .push((avatar_id.uuid(), groups.len(), *list_in_profile)),
        Event::AvatarPicks { target_id, picks } => got.picks.push((*target_id, picks.clone())),
        Event::AvatarNotes { target_id, notes } => got.notes.push((*target_id, notes.clone())),
        Event::AvatarClassifieds {
            target_id,
            classifieds,
        } => got.classifieds.push((*target_id, classifieds.clone())),
        Event::PickInfo(info) => got.pick_infos.push((**info).clone()),
        Event::ClassifiedInfo(info) => got.classified_infos.push((**info).clone()),
        Event::GenericMessage(message) | Event::LargeGenericMessage(message) => {
            got.generic.push(message.method.clone());
        }
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            got.alerts.push(message.clone());
        }
        Event::MoneyBalance(balance) => got.money.push(balance.clone()),
        Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
        Event::DisplayNameUpdate(_) => {
            got.display_name_updates = got.display_name_updates.saturating_add(1);
        }
        Event::SetDisplayNameReply(_) => {
            got.set_display_name_replies = got.set_display_name_replies.saturating_add(1);
        }
        Event::ClassifiedClickStats(stats) => got.click_stats.push(*stats),
        Event::ProfileImageUploaded { image, asset_id } => got.images.push((*image, *asset_id)),
        _ => return None,
    }
    Some(())
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most.
async fn watch_for(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Got) -> bool,
) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        if done_at.is_none() && done(&got) {
            done_at = Some(Instant::now());
        }
        let remaining = done_at.map_or_else(
            || limit.saturating_sub(started.elapsed()),
            |at| quiet.saturating_sub(at.elapsed()),
        );
        if remaining.is_zero() {
            return Ok(got);
        }
        match session
            .wait_for(remaining, |event| fold(&mut got, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(other) => return Err(other),
        }
    }
}

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    watch_for(session, window, window, |_got| false).await
}

/// The ids and texts a metric names by what they are rather than by their
/// value.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// Every way the grid writes A's name.
    a_names: Vec<String>,
    /// Every way the grid writes B's name.
    b_names: Vec<String>,
    /// This run's tag, which every census text begins with.
    tag: String,
    /// The texts the census wrote, by what a metric calls them.
    texts: Vec<(String, &'static str)>,
    /// Ids the census made or was given, by what a metric calls them.
    named: Vec<(Uuid, &'static str)>,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else {
            self.named
                .iter()
                .rev()
                .find(|(named, _what)| *named == id)
                .map_or("another id", |(_named, what)| *what)
        }
    }

    /// A census text named `what`, remembered so a metric can name it.
    fn text(&mut self, what: &'static str) -> String {
        let text = format!("{}-{what}", self.tag);
        self.texts.push((text.clone(), what));
        text
    }

    /// A census text of `length` bytes named `what`.
    fn long_text(&mut self, what: &'static str, length: usize) -> String {
        let mut text = format!("{}-{what}-", self.tag);
        while text.len() < length {
            text.push('x');
        }
        self.texts.push((text.clone(), what));
        text
    }

    /// What a free text is: one of the census's (whole or cut), or else only
    /// how long it is — an avatar's own profile text is not the census's to
    /// record.
    fn free_text(&self, text: &str) -> String {
        if text.is_empty() {
            return "empty".to_owned();
        }
        for (written, what) in &self.texts {
            if text == written {
                return format!("the {what} text");
            }
            if written.starts_with(text) {
                return format!("the {what} text cut to {} bytes", text.len());
            }
        }
        if text.starts_with(&self.tag) {
            return format!("a census text of {} bytes", text.len());
        }
        format!("{} bytes of the avatar's own", text.len())
    }

    /// `text` with nothing in it that is one run's alone: the avatars' names,
    /// the tag, every id and every date.
    fn scrubbed(&self, text: &str) -> String {
        let mut out = text.replace(&self.tag, "<tag>");
        let mut names: Vec<(&str, &str)> = self
            .a_names
            .iter()
            .map(|name| (name.as_str(), "<A>"))
            .chain(self.b_names.iter().map(|name| (name.as_str(), "<B>")))
            .collect();
        names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
        for (name, word) in names {
            if !name.is_empty() {
                out = out.replace(name, word);
            }
        }
        out = without_dates(&without_ids(&out));
        if out.len() > 200 {
            let cut = (0..=200)
                .rev()
                .find(|at| out.is_char_boundary(*at))
                .unwrap_or(0);
            out.truncate(cut);
            out.push('…');
        }
        out
    }

    /// Whose name `name` is.
    fn name_text(&self, name: &str) -> String {
        let is = |names: &[String]| names.iter().any(|known| name.eq_ignore_ascii_case(known));
        if name.is_empty() {
            "no name".to_owned()
        } else if is(&self.a_names) {
            "A's name".to_owned()
        } else if is(&self.b_names) {
            "B's name".to_owned()
        } else {
            format!("the name {:?}", self.scrubbed(name))
        }
    }

    /// A list of picks or classifieds as a metric states it: the census's own
    /// entries by name, and whether the avatar has others of its own (a grid
    /// that cannot delete keeps every earlier run's).
    fn list_text(&self, entries: &[(Uuid, &str)]) -> String {
        let mut named: Vec<String> = Vec::new();
        let mut others = false;
        for (id, name) in entries {
            match self.id_text(*id) {
                "another id" => others = true,
                what => named.push(format!("{what} {}", self.free_text(name))),
            }
        }
        if others {
            named.push("some of the avatar's own".to_owned());
        }
        format!("[{}]", named.join(", "))
    }

    /// A UDP profile read as a metric states it.
    fn properties_text(&self, props: &AvatarProperties) -> String {
        format!(
            "{}: about {}, first life about {}, image {}, first life image {}, partner {}, \
             born {}, url {:?}, charter {:?}, flags {:#x}",
            self.id_text(props.avatar_id.uuid()),
            self.free_text(&props.about_text),
            self.free_text(&props.fl_about_text),
            self.id_text(props.image_id.uuid()),
            self.id_text(props.fl_image_id.uuid()),
            props
                .partner_id
                .map_or("nobody", |partner| self.id_text(partner.uuid())),
            digits_as_d(&props.born_on),
            self.scrubbed(&props.profile_url),
            self.scrubbed(&props.charter_member),
            props.flags,
        )
    }

    /// An interests read as a metric states it.
    fn interests_text(&self, interests: &AvatarInterests) -> String {
        format!(
            "{}: want to {:#x} {}, skills {:#x} {}, languages {}",
            self.id_text(interests.avatar_id.uuid()),
            interests.want_to_mask,
            self.free_text(&interests.want_to_text),
            interests.skills_mask,
            self.free_text(&interests.skills_text),
            self.free_text(&interests.languages_text),
        )
    }

    /// A pick read as a metric states it.
    fn pick_text(&self, pick: &PickInfo) -> String {
        format!(
            "{} by {}: top {}, parcel {}, name {}, description {}, snapshot {}, user {}, \
             parcel name {}, region {}, position {}, sort {}, enabled {}",
            self.id_text(pick.pick_id.uuid()),
            self.id_text(pick.creator_id.uuid()),
            pick.top_pick,
            some_or_none(!pick.parcel_id.uuid().is_nil()),
            self.free_text(&pick.name),
            self.free_text(&pick.description),
            pick.snapshot_id
                .map_or("none", |snapshot| self.id_text(snapshot.uuid())),
            self.name_text(&pick.user),
            some_or_none(!pick.original_name.is_empty()),
            some_or_none(pick.sim_name.is_some()),
            position_text(
                pick.pos_global.x(),
                pick.pos_global.y(),
                pick.pos_global.z()
            ),
            pick.sort_order,
            pick.enabled,
        )
    }

    /// A classified read as a metric states it.
    fn classified_text(&self, info: &ClassifiedInfo) -> String {
        format!(
            "{} by {}: category {:?}, name {}, description {}, parcel {}, estate {}, \
             snapshot {}, region {}, position {}, parcel name {}, flags {:#x}, price {}, \
             created {}, expires {}",
            self.id_text(info.classified_id.uuid()),
            self.id_text(info.creator_id.uuid()),
            info.category,
            self.free_text(&info.name),
            self.free_text(&info.description),
            some_or_none(!info.parcel_id.uuid().is_nil()),
            info.parent_estate,
            info.snapshot_id
                .map_or("none", |snapshot| self.id_text(snapshot.uuid())),
            some_or_none(info.sim_name.is_some()),
            position_text(
                info.pos_global.x(),
                info.pos_global.y(),
                info.pos_global.z()
            ),
            some_or_none(!info.parcel_name.is_empty()),
            info.classified_flags,
            info.price_for_listing.0,
            time_text(info.creation_date),
            if info.expiration_date > info.creation_date {
                "after the creation"
            } else {
                time_text(info.expiration_date)
            },
        )
    }

    /// Everything `got` holds, as a metric states it; `nothing` where it holds
    /// nothing.
    fn got_text(&self, got: &Got) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.extend(
            got.properties
                .iter()
                .map(|props| format!("profile {}", self.properties_text(props))),
        );
        parts.extend(
            got.interests
                .iter()
                .map(|interests| format!("interests {}", self.interests_text(interests))),
        );
        // How many groups an avatar is in is the account's, not the grid's,
        // and so is how many messages the grid splits them into: say once
        // whether there were any.
        let mut group_lists: Vec<(Uuid, bool, bool)> = Vec::new();
        for (who, count, listed) in &got.groups {
            match group_lists
                .iter_mut()
                .find(|(seen, _any, _listed)| seen == who)
            {
                Some((_seen, any, _listed)) => *any |= *count > 0,
                None => group_lists.push((*who, *count > 0, *listed)),
            }
        }
        parts.extend(group_lists.iter().map(|(who, any, listed)| {
            format!(
                "groups of {}: {}, shown in the profile {listed}",
                self.id_text(*who),
                if *any { "some" } else { "none" }
            )
        }));
        parts.extend(got.picks.iter().map(|(who, picks)| {
            let entries: Vec<(Uuid, &str)> = picks
                .iter()
                .map(|pick| (pick.pick_id.uuid(), pick.name.as_str()))
                .collect();
            format!(
                "picks of {}: {}",
                self.id_text(*who),
                self.list_text(&entries)
            )
        }));
        parts.extend(got.notes.iter().map(|(who, notes)| {
            format!(
                "notes about {}: {}",
                self.id_text(*who),
                self.free_text(notes)
            )
        }));
        parts.extend(got.classifieds.iter().map(|(who, classifieds)| {
            let entries: Vec<(Uuid, &str)> = classifieds
                .iter()
                .map(|classified| (classified.classified_id.uuid(), classified.name.as_str()))
                .collect();
            format!(
                "classifieds of {}: {}",
                self.id_text(*who),
                self.list_text(&entries)
            )
        }));
        parts.extend(
            got.pick_infos
                .iter()
                .map(|pick| format!("pick {}", self.pick_text(pick))),
        );
        parts.extend(
            got.classified_infos
                .iter()
                .map(|info| format!("classified {}", self.classified_text(info))),
        );
        parts.extend(
            got.generic
                .iter()
                .map(|method| format!("generic message {method}")),
        );
        parts.extend(
            got.alerts
                .iter()
                .map(|alert| format!("alert {:?}", self.scrubbed(alert))),
        );
        parts.extend(got.money.iter().map(|balance| {
            format!(
                "balance{}: {}, {:?}",
                if balance.success { "" } else { " (failed)" },
                balance.transaction.as_ref().map_or_else(
                    || "no transaction".to_owned(),
                    |transaction| format!(
                        "type {}, amount {}",
                        transaction.transaction_type, transaction.amount.0
                    )
                ),
                self.scrubbed(&balance.description),
            )
        }));
        parts.extend(got.ims.iter().map(|im| {
            format!(
                "IM {:?} from {}: {:?}",
                im.dialog,
                self.id_text(im.from_agent_id.uuid()),
                self.scrubbed(&im.message)
            )
        }));
        parts.extend(got.click_stats.iter().map(|stats| {
            format!(
                "click counts of {}: teleport {}, map {}, profile {}, {}",
                self.id_text(stats.classified_id.uuid()),
                stats.teleport,
                stats.map,
                stats.profile,
                stats.search.map_or_else(
                    || "none from search".to_owned(),
                    |search| format!(
                        "from search teleport {}, map {}, profile {}",
                        search.teleport, search.map, search.profile
                    )
                ),
            )
        }));
        parts.extend(got.images.iter().map(|(image, asset)| {
            format!(
                "{image:?} image uploaded: {}",
                asset.map_or("no asset", |asset| self.id_text(asset.uuid()))
            )
        }));
        if got.display_name_updates > 0 {
            parts.push(format!(
                "{} display-name update(s)",
                got.display_name_updates
            ));
        }
        if got.set_display_name_replies > 0 {
            parts.push(format!(
                "{} display-name set answer(s)",
                got.set_display_name_replies
            ));
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join(" | ")
        }
    }

    /// An LLSD value read from a capability, as a metric states it: ids by
    /// what they are, the census's texts by name, an avatar's own texts by
    /// length, and every key in order.
    fn llsd_text(&self, value: &Llsd) -> String {
        match value {
            Llsd::Map(map) => {
                let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
                entries.sort_unstable_by_key(|(key, _value)| *key);
                let entries: Vec<String> = entries
                    .into_iter()
                    .map(|(key, value)| format!("{key}={}", self.field_text(key, value)))
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            Llsd::Array(items) => {
                let items: Vec<String> = items.iter().map(|item| self.llsd_text(item)).collect();
                format!("[{}]", items.join(", "))
            }
            other => self.field_text("", other),
        }
    }

    /// One field of a capability's answer, as a metric states it.
    fn field_text(&self, key: &str, value: &Llsd) -> String {
        match value {
            Llsd::Uuid(id) => self.id_text(*id).to_owned(),
            Llsd::String(text) if text.starts_with("http") => "a URL".to_owned(),
            Llsd::String(text) if is_free_text(key) => self.free_text(text),
            Llsd::String(text) => match Uuid::parse_str(text) {
                Ok(id) => format!("{} as a string", self.id_text(id)),
                Err(_) => format!("{:?}", self.scrubbed(text)),
            },
            Llsd::Date(_) => "a date".to_owned(),
            Llsd::Boolean(flag) => flag.to_string(),
            Llsd::Integer(number) => number.to_string(),
            Llsd::Real(number) => number.to_string(),
            Llsd::Map(_) | Llsd::Array(_) => self.llsd_text(value),
            other => shape_of(other),
        }
    }
}

/// Whether a capability's field `key` holds an avatar's free text.
fn is_free_text(key: &str) -> bool {
    key.contains("about")
        || key == "notes"
        || key == "name"
        || key == "message"
        || key == "description"
}

/// An L$ amount as a signed count, so a charge and a refund both read.
fn signed(amount: u64) -> i64 {
    i64::try_from(amount).unwrap_or(i64::MAX)
}

/// `some` or `none`.
const fn some_or_none(some: bool) -> &'static str {
    if some { "some" } else { "none" }
}

/// A global position: zero, or set.
fn position_text(x: f64, y: f64, z: f64) -> &'static str {
    if x == 0.0 && y == 0.0 && z == 0.0 {
        "zero"
    } else {
        "set"
    }
}

/// A Unix time: zero, or a time.
const fn time_text(time: u32) -> &'static str {
    if time == 0 { "zero" } else { "a time" }
}

/// `text` with every digit written `d`: the form of a date without the date.
fn digits_as_d(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { 'd' } else { c })
        .collect()
}

/// What a capability answered.
#[derive(Debug)]
struct Answered {
    /// The HTTP status.
    status: u16,
    /// The body as text.
    text: String,
}

impl Answered {
    /// The body as LLSD, where it is LLSD.
    fn llsd(&self) -> Option<Llsd> {
        if self.text.contains("<llsd") {
            parse_llsd_xml(&self.text).ok()
        } else {
            None
        }
    }

    /// The answer as a metric states it: the status, and the body by
    /// [`Scene::llsd_text`] where it is LLSD, its first words where it is not.
    fn text(&self, scene: &Scene) -> String {
        let body = match self.llsd() {
            Some(llsd) => scene.llsd_text(&llsd),
            None if self.text.trim().is_empty() => "no body".to_owned(),
            None => {
                let words: Vec<&str> = self.text.split_whitespace().take(6).collect();
                format!("{:?}", scene.scrubbed(&words.join(" ")))
            }
        };
        format!("{}, {body}", self.status)
    }
}

/// How many times the census sends a capability request that did not reach
/// the grid (a transport failure, not an answer).
const SEND_ATTEMPTS: u32 = 3;

/// Sends `request` and reads what came back; a request that does not reach
/// the grid is sent again, up to [`SEND_ATTEMPTS`] times, since a census
/// measures answers, not the network on the way.
async fn answered(request: reqwest::RequestBuilder) -> Result<Answered, TestFailure> {
    let mut last_error = String::new();
    for _attempt in 0..SEND_ATTEMPTS {
        let Some(attempt) = request.try_clone() else {
            break;
        };
        match attempt.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                return Ok(Answered { status, text });
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(TestFailure::Assertion(format!(
        "a capability request failed: {last_error}"
    )))
}

/// An LLSD map of `entries`, as the XML a capability takes.
fn llsd_map(entries: Vec<(&str, Llsd)>) -> String {
    Llsd::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<HashMap<String, Llsd>>(),
    )
    .to_llsd_xml()
}

/// Something of A's the census changed and puts back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// The profile texts.
    Profile,
    /// The interests.
    Interests,
    /// The notes about B.
    Notes,
    /// The `hide_age` flag.
    HideAge,
    /// The first-life image.
    FirstLifeImage,
}

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A, which reads and writes.
    a: &'a mut Session,
    /// Avatar B, which is read.
    b: &'a mut Session,
    /// The ids and texts metrics name.
    scene: Scene,
    /// The grid the census runs on.
    grid: Grid,
    /// An HTTP client for the capabilities the census asks itself.
    http: reqwest::Client,
    /// A's profile as the census found it.
    found: Option<AvatarProperties>,
    /// A's interests as the census found them.
    found_interests: Option<AvatarInterests>,
    /// A's notes about B as the census found them over the capability.
    found_notes: Option<String>,
    /// A's `hide_age` as the census found it over the capability.
    found_hide_age: Option<bool>,
    /// A's first-life image as the census found it over the capability.
    found_fl_image: Option<Uuid>,
    /// What the census changed and is to put back.
    changed: Vec<Change>,
    /// The picks the census made.
    picks: Vec<PickKey>,
    /// The classifieds the census made.
    classifieds: Vec<ClassifiedKey>,
}

impl Census<'_> {
    /// A capability's URL on A's region, waited for as long as the capability
    /// map may take; `None` where the region does not grant it.
    async fn cap(&self, name: &str) -> Option<String> {
        let started = Instant::now();
        loop {
            if let Some(url) = self.a.cap(name) {
                return Some(url);
            }
            if self.a.capability_names().is_empty() && started.elapsed() < CAP_WAIT {
                tokio::time::sleep(Duration::from_millis(250)).await;
            } else {
                return None;
            }
        }
    }

    /// GETs `who`'s profile over `AgentProfile`.
    async fn cap_read(&self, who: Uuid) -> Result<Option<Answered>, TestFailure> {
        let Some(url) = self.cap(AGENT_PROFILE).await else {
            return Ok(None);
        };
        answered(
            self.http
                .get(format!("{url}/{who}"))
                .header("Accept", "application/llsd+xml"),
        )
        .await
        .map(Some)
    }

    /// PUTs `entries` to `who`'s profile over `AgentProfile`.
    async fn cap_put(
        &self,
        who: Uuid,
        entries: Vec<(&str, Llsd)>,
    ) -> Result<Option<Answered>, TestFailure> {
        let Some(url) = self.cap(AGENT_PROFILE).await else {
            return Ok(None);
        };
        answered(
            self.http
                .put(format!("{url}/{who}"))
                .header("Content-Type", "application/llsd+xml")
                .header("Accept", "application/llsd+xml")
                .body(llsd_map(entries)),
        )
        .await
        .map(Some)
    }

    /// What B reads of `who`'s age over B's own `AgentProfile` capability:
    /// `member_since` and `hide_age`.
    async fn seen_by_b(&self, who: Uuid) -> Result<String, TestFailure> {
        let Some(url) = self.b.cap(AGENT_PROFILE) else {
            return Ok(NO_CAPABILITY.to_owned());
        };
        let answer = answered(
            self.http
                .get(format!("{url}/{who}"))
                .header("Accept", "application/llsd+xml"),
        )
        .await?;
        Ok(match answer.llsd() {
            Some(llsd) => format!(
                "member since {}, hide age {}",
                llsd.get("member_since").map_or_else(
                    || "absent".to_owned(),
                    |value| self.scene.field_text("member_since", value)
                ),
                llsd.get("hide_age").map_or_else(
                    || "absent".to_owned(),
                    |value| self.scene.field_text("hide_age", value)
                ),
            ),
            None => format!("{}, not LLSD", answer.status),
        })
    }

    /// One field of `who`'s profile over the capability, as a metric states
    /// it; `no capability` where there is none.
    async fn cap_field(&self, who: Uuid, key: &str) -> Result<String, TestFailure> {
        Ok(match self.cap_read(who).await? {
            None => NO_CAPABILITY.to_owned(),
            Some(answer) => match answer.llsd() {
                Some(llsd) => llsd.get(key).map_or_else(
                    || "absent".to_owned(),
                    |value| self.scene.field_text(key, value),
                ),
                None => format!("{}, not LLSD", answer.status),
            },
        })
    }

    /// Records what B was told while A's leg ran, under `<leg>_told_b`.
    async fn record_b(&mut self, metrics: &mut Metrics, leg: &str) -> Result<(), TestFailure> {
        let got = watch(self.b, DRAIN).await?;
        metrics.set(&format!("{leg}_told_b"), self.scene.got_text(&got));
        Ok(())
    }

    /// A asks for `who`'s profile over UDP; records what it brought.
    async fn udp_read(
        &mut self,
        metrics: &mut Metrics,
        leg: &str,
        who: Uuid,
    ) -> Result<Got, TestFailure> {
        let (agent_id, session_id) = ids_of(self.a)?;
        send_raw(
            self.a,
            AnyMessage::AvatarPropertiesRequest(AvatarPropertiesRequest {
                agent_data: AvatarPropertiesRequestAgentDataBlock {
                    agent_id,
                    session_id,
                    avatar_id: who,
                },
            }),
        )
        .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| got.has_properties(who)).await?;
        metrics.set(&format!("{leg}_told_a"), self.scene.got_text(&got));
        Ok(got)
    }

    /// `session` sends the generic message `method` about `who` over UDP, as
    /// the reference does (both NUL-terminated), whichever road the client
    /// would take.
    async fn generic_raw(session: &Session, method: &str, who: Uuid) -> Result<(), TestFailure> {
        let (agent_id, session_id) = ids_of(session)?;
        send_raw(
            session,
            AnyMessage::GenericMessage(GenericMessage {
                agent_data: GenericMessageAgentDataBlock {
                    agent_id,
                    session_id,
                    transaction_id: Uuid::nil(),
                },
                method_data: GenericMessageMethodDataBlock {
                    method: nul_terminated(method),
                    invoice: Uuid::nil(),
                },
                param_list: vec![GenericMessageParamListBlock {
                    parameter: nul_terminated(&who.to_string()),
                }],
            }),
        )
        .await
    }

    /// A reads `who`'s profile over the capability; records it.
    async fn cap_profile(
        &self,
        metrics: &mut Metrics,
        leg: &str,
        who: Uuid,
    ) -> Result<Option<Llsd>, TestFailure> {
        let answer = self.cap_read(who).await?;
        let (text, llsd) = match answer {
            None => (NO_CAPABILITY.to_owned(), None),
            Some(answer) => (answer.text(&self.scene), answer.llsd()),
        };
        metrics.set(&format!("{leg}_answer"), text);
        metrics.set(
            &format!("{leg}_shape"),
            llsd.as_ref()
                .map_or_else(|| NO_CAPABILITY.to_owned(), shape_of),
        );
        Ok(llsd)
    }

    /// Leg 1 and 2: both profiles, both roads.
    async fn reads(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let (a, b) = (self.scene.a, self.scene.b);
        self.udp_read(metrics, "read_other_udp", b).await?;
        self.record_b(metrics, "read_other_udp").await?;
        self.cap_profile(metrics, "read_other_cap", b).await?;
        self.record_b(metrics, "read_other_cap").await?;
        let own = self.udp_read(metrics, "read_own_udp", a).await?;
        self.found = own
            .properties
            .iter()
            .find(|props| props.avatar_id.uuid() == a)
            .cloned();
        self.found_interests = own
            .interests
            .iter()
            .find(|interests| interests.avatar_id.uuid() == a)
            .cloned();
        let own_cap = self.cap_profile(metrics, "read_own_cap", a).await?;
        if let Some(llsd) = own_cap {
            self.found_hide_age = match llsd.get("hide_age") {
                Some(Llsd::Boolean(flag)) => Some(*flag),
                _ => None,
            };
            self.found_fl_image = match llsd.get("fl_image_id") {
                Some(Llsd::Uuid(id)) => Some(*id),
                _ => None,
            };
        }
        Ok(())
    }

    /// Leg 3: an edit over UDP, read back both ways.
    async fn udp_edit(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let a = self.scene.a;
        let Some(found) = self.found.clone() else {
            metrics.set("edit_udp_told_a", "no profile to edit");
            metrics.set("edit_udp_reads_udp", "no profile to edit");
            metrics.set("edit_udp_reads_cap", "no profile to edit");
            return Ok(());
        };
        let about = self.scene.text("udp about");
        let fl_about = self.scene.text("udp first life about");
        self.changed.push(Change::Profile);
        let (agent_id, session_id) = ids_of(self.a)?;
        let update = profile_update(&found);
        send_raw(
            self.a,
            AnyMessage::AvatarPropertiesUpdate(AvatarPropertiesUpdate {
                agent_data: AvatarPropertiesUpdateAgentDataBlock {
                    agent_id,
                    session_id,
                },
                properties_data: AvatarPropertiesUpdatePropertiesDataBlock {
                    image_id: update.image_id.uuid(),
                    fl_image_id: update.fl_image_id.uuid(),
                    about_text: nul_terminated(&about),
                    fl_about_text: nul_terminated(&fl_about),
                    allow_publish: update.allow_publish,
                    mature_publish: update.mature_publish,
                    profile_url: nul_terminated(&update.profile_url),
                },
            }),
        )
        .await?;
        let told = watch(self.a, SETTLE).await?;
        metrics.set("edit_udp_told_a", self.scene.got_text(&told));
        self.record_b(metrics, "edit_udp").await?;
        let read = self.udp_read(metrics, "edit_udp_read", a).await?;
        metrics.set(
            "edit_udp_reads_udp",
            read.properties
                .iter()
                .find(|props| props.avatar_id.uuid() == a)
                .map_or_else(
                    || "no profile".to_owned(),
                    |props| {
                        format!(
                            "about {}, first life about {}",
                            self.scene.free_text(&props.about_text),
                            self.scene.free_text(&props.fl_about_text)
                        )
                    },
                ),
        );
        let about_cap = self.cap_field(a, "sl_about_text").await?;
        let fl_about_cap = self.cap_field(a, "fl_about_text").await?;
        metrics.set(
            "edit_udp_reads_cap",
            if about_cap == NO_CAPABILITY {
                NO_CAPABILITY.to_owned()
            } else {
                format!("about {about_cap}, first life about {fl_about_cap}")
            },
        );
        Ok(())
    }

    /// Leg 4: edits over the capability, read back both ways.
    async fn cap_edits(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let a = self.scene.a;
        // One key.
        let about = self.scene.text("cap about");
        let put = self
            .cap_put(a, vec![("sl_about_text", Llsd::String(about))])
            .await?;
        if put.is_some() {
            self.changed.push(Change::Profile);
        }
        metrics.set("put_one_answer", self.answer_text(put.as_ref()));
        tokio::time::sleep(SETTLE).await;
        metrics.set(
            "put_one_reads_cap",
            self.cap_field(a, "sl_about_text").await?,
        );
        let read = self.udp_read(metrics, "put_one_read", a).await?;
        metrics.set("put_one_reads_udp", self.about_of(&read, a));

        // Two keys.
        let about = self.scene.text("cap two about");
        let fl_about = self.scene.text("cap two first life about");
        let put = self
            .cap_put(
                a,
                vec![
                    ("sl_about_text", Llsd::String(about)),
                    ("fl_about_text", Llsd::String(fl_about)),
                ],
            )
            .await?;
        metrics.set("put_two_answer", self.answer_text(put.as_ref()));
        tokio::time::sleep(SETTLE).await;
        let about_cap = self.cap_field(a, "sl_about_text").await?;
        let fl_about_cap = self.cap_field(a, "fl_about_text").await?;
        metrics.set(
            "put_two_reads_cap",
            if about_cap == NO_CAPABILITY {
                NO_CAPABILITY.to_owned()
            } else {
                format!("about {about_cap}, first life about {fl_about_cap}")
            },
        );

        // hide_age, the flag only the capability carries.
        let hide = !self.found_hide_age.unwrap_or(false);
        let put = self
            .cap_put(a, vec![("hide_age", Llsd::Boolean(hide))])
            .await?;
        if put.is_some() {
            self.changed.push(Change::HideAge);
        }
        metrics.set("put_hide_age_answer", self.answer_text(put.as_ref()));
        tokio::time::sleep(SETTLE).await;
        let read = self.cap_field(a, "hide_age").await?;
        metrics.set(
            "put_hide_age_reads",
            if read == NO_CAPABILITY {
                read
            } else if read == hide.to_string() {
                "the flag as put".to_owned()
            } else {
                format!("{read}, not as put")
            },
        );
        metrics.set("put_hide_age_seen_by_b", self.seen_by_b(a).await?);

        // A text longer than the UDP field.
        let long = self.scene.long_text("long about", LONG_TEXT_LENGTH);
        let put = self
            .cap_put(a, vec![("sl_about_text", Llsd::String(long))])
            .await?;
        metrics.set("put_long_answer", self.answer_text(put.as_ref()));
        tokio::time::sleep(SETTLE).await;
        metrics.set(
            "put_long_reads_cap",
            self.cap_field(a, "sl_about_text").await?,
        );
        let read = self.udp_read(metrics, "put_long_read", a).await?;
        metrics.set("put_long_reads_udp", self.about_of(&read, a));
        Ok(())
    }

    /// A capability's answer as a metric states it.
    fn answer_text(&self, answer: Option<&Answered>) -> String {
        answer.map_or_else(
            || NO_CAPABILITY.to_owned(),
            |answer| answer.text(&self.scene),
        )
    }

    /// The about text of `who` among the profiles `got` holds.
    fn about_of(&self, got: &Got, who: Uuid) -> String {
        got.properties
            .iter()
            .find(|props| props.avatar_id.uuid() == who)
            .map_or_else(
                || "no profile".to_owned(),
                |props| self.scene.free_text(&props.about_text),
            )
    }

    /// Leg 5: interests over UDP.
    async fn interests(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let a = self.scene.a;
        let want = self.scene.text("want to");
        let found = self.found_interests.clone();
        let update = found
            .as_ref()
            .map_or_else(InterestsUpdate::default, |found| InterestsUpdate {
                want_to_mask: found.want_to_mask,
                want_to_text: found.want_to_text.clone(),
                skills_mask: found.skills_mask,
                skills_text: found.skills_text.clone(),
                languages_text: found.languages_text.clone(),
            });
        self.changed.push(Change::Interests);
        self.a
            .send(Command::UpdateInterests(InterestsUpdate {
                want_to_text: want,
                want_to_mask: update.want_to_mask ^ 0x1,
                ..update
            }))
            .await?;
        let told = watch(self.a, SETTLE).await?;
        metrics.set("interests_told_a", self.scene.got_text(&told));
        let read = self.udp_read(metrics, "interests_read", a).await?;
        metrics.set(
            "interests_reads",
            read.interests
                .iter()
                .find(|interests| interests.avatar_id.uuid() == a)
                .map_or_else(
                    || "no interests".to_owned(),
                    |interests| {
                        format!(
                            "want to {}, mask {}",
                            self.scene.free_text(&interests.want_to_text),
                            if found.as_ref().is_some_and(|found| {
                                interests.want_to_mask == found.want_to_mask ^ 0x1
                            }) {
                                "as sent"
                            } else {
                                "not as sent"
                            }
                        )
                    },
                ),
        );
        Ok(())
    }

    /// Leg 6: notes about B.
    async fn notes(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let b = self.scene.b;
        self.found_notes = match self.cap_read(b).await? {
            Some(answer) => answer.llsd().and_then(|llsd| match llsd.get("notes") {
                Some(Llsd::String(notes)) => Some(notes.clone()),
                _ => None,
            }),
            None => None,
        };
        Self::generic_raw(self.a, "avatarnotesrequest", b).await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.notes.iter().any(|(who, _notes)| *who == b)
        })
        .await?;
        metrics.set("notes_ask_told_a", self.scene.got_text(&got));
        let udp_notes_before = got
            .notes
            .iter()
            .find(|(who, _notes)| *who == b)
            .map(|(_who, notes)| notes.clone());
        if self.found_notes.is_none() {
            self.found_notes.clone_from(&udp_notes_before);
        }

        let note = self.scene.text("udp note");
        self.changed.push(Change::Notes);
        let (agent_id, session_id) = ids_of(self.a)?;
        send_raw(
            self.a,
            AnyMessage::AvatarNotesUpdate(AvatarNotesUpdate {
                agent_data: AvatarNotesUpdateAgentDataBlock {
                    agent_id,
                    session_id,
                },
                data: AvatarNotesUpdateDataBlock {
                    target_id: b,
                    notes: nul_terminated(&note),
                },
            }),
        )
        .await?;
        let told = watch(self.a, SETTLE).await?;
        metrics.set("notes_udp_told_a", self.scene.got_text(&told));
        self.record_b(metrics, "notes_udp").await?;
        Self::generic_raw(self.a, "avatarnotesrequest", b).await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.notes.iter().any(|(who, _notes)| *who == b)
        })
        .await?;
        metrics.set("notes_udp_reads_udp", self.notes_text(&got, b));
        metrics.set("notes_udp_reads_cap", self.cap_field(b, "notes").await?);

        let note = self.scene.text("cap note");
        let put = self.cap_put(b, vec![("notes", Llsd::String(note))]).await?;
        metrics.set("notes_cap_answer", self.answer_text(put.as_ref()));
        tokio::time::sleep(SETTLE).await;
        self.record_b(metrics, "notes_cap").await?;
        metrics.set("notes_cap_reads_cap", self.cap_field(b, "notes").await?);
        Self::generic_raw(self.a, "avatarnotesrequest", b).await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.notes.iter().any(|(who, _notes)| *who == b)
        })
        .await?;
        metrics.set("notes_cap_reads_udp", self.notes_text(&got, b));
        Ok(())
    }

    /// The notes about `who` among what `got` holds.
    fn notes_text(&self, got: &Got, who: Uuid) -> String {
        got.notes
            .iter()
            .find(|(about, _notes)| *about == who)
            .map_or_else(
                || "no answer".to_owned(),
                |(_who, notes)| self.scene.free_text(notes),
            )
    }

    /// Leg 7: picks.
    async fn picks(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let (a, b) = (self.scene.a, self.scene.b);
        for (leg, who) in [("picks_own_ask", a), ("picks_other_ask", b)] {
            Self::generic_raw(self.a, "avatarpicksrequest", who).await?;
            let got = watch_for(self.a, LISTEN, QUIET, |got| {
                got.picks.iter().any(|(whose, _picks)| *whose == who)
            })
            .await?;
            metrics.set(&format!("{leg}_told_a"), self.scene.got_text(&got));
        }

        let pick = PickKey::from(Uuid::new_v4());
        self.scene.named.push((pick.uuid(), "the pick"));
        self.picks.push(pick);
        let name = self.scene.text("pick");
        let description = self.scene.text("pick description");
        self.a
            .send(Command::UpdatePick(PickUpdate {
                pick_id: pick,
                name: name.clone(),
                description,
                ..PickUpdate::default()
            }))
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.pick_infos.iter().any(|info| info.pick_id == pick)
        })
        .await?;
        metrics.set("pick_create_told_a", self.scene.got_text(&got));
        self.record_b(metrics, "pick_create").await?;

        self.a
            .send(Command::RequestPickInfo {
                creator_id: AgentKey::from(a),
                pick_id: pick,
            })
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.pick_infos.iter().any(|info| info.pick_id == pick)
        })
        .await?;
        metrics.set("pick_read_told_a", self.scene.got_text(&got));
        metrics.set("pick_listed_cap", self.cap_field(a, "picks").await?);

        let description = self.scene.text("pick edited description");
        self.a
            .send(Command::UpdatePick(PickUpdate {
                pick_id: pick,
                name,
                description,
                ..PickUpdate::default()
            }))
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.pick_infos.iter().any(|info| info.pick_id == pick)
        })
        .await?;
        metrics.set("pick_edit_told_a", self.scene.got_text(&got));

        Self::generic_raw(self.b, "avatarpicksrequest", a).await?;
        let got = watch_for(self.b, LISTEN, QUIET, |got| {
            got.picks.iter().any(|(whose, _picks)| *whose == a)
        })
        .await?;
        metrics.set("pick_other_reads_told_b", self.scene.got_text(&got));
        self.b
            .send(Command::RequestPickInfo {
                creator_id: AgentKey::from(a),
                pick_id: pick,
            })
            .await?;
        let got = watch_for(self.b, LISTEN, QUIET, |got| {
            got.pick_infos.iter().any(|info| info.pick_id == pick)
        })
        .await?;
        metrics.set("pick_other_reads_info_told_b", self.scene.got_text(&got));

        self.a.send(Command::DeletePick(pick)).await?;
        let got = watch(self.a, SETTLE).await?;
        metrics.set("pick_delete_told_a", self.scene.got_text(&got));
        self.picks.retain(|made| *made != pick);
        Self::generic_raw(self.a, "avatarpicksrequest", a).await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.picks.iter().any(|(whose, _picks)| *whose == a)
        })
        .await?;
        metrics.set("pick_deleted_list_told_a", self.scene.got_text(&got));
        metrics.set("pick_deleted_cap", self.cap_field(a, "picks").await?);
        Ok(())
    }

    /// Leg 8: classifieds.
    async fn classifieds(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let (a, b) = (self.scene.a, self.scene.b);
        for (leg, who) in [("classifieds_own_ask", a), ("classifieds_other_ask", b)] {
            self.a
                .send(Command::RequestAvatarClassifieds(AgentKey::from(who)))
                .await?;
            let got = watch_for(self.a, LISTEN, QUIET, |got| {
                got.classifieds.iter().any(|(whose, _list)| *whose == who)
            })
            .await?;
            metrics.set(&format!("{leg}_told_a"), self.scene.got_text(&got));
        }

        self.a.send(Command::RequestMoneyBalance).await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| !got.money.is_empty()).await?;
        let mut balance = got.money.last().map(|money| money.balance.0);

        for (leg, what, price) in [
            ("classified_free", "the free classified", 0),
            ("classified_paid", "the paid classified", LISTING_PRICE),
        ] {
            // OpenSim cannot delete a classified: there a run lists under a
            // fixed id, and edits the one an earlier run left.
            let classified = if is_opensim(self.grid) {
                ClassifiedKey::from(if price == 0 {
                    OPENSIM_FREE_CLASSIFIED
                } else {
                    OPENSIM_PAID_CLASSIFIED
                })
            } else {
                ClassifiedKey::from(Uuid::new_v4())
            };
            self.scene.named.push((classified.uuid(), what));
            self.classifieds.push(classified);
            let name = self.scene.text(if price == 0 {
                "free classified"
            } else {
                "paid classified"
            });
            let description = self.scene.text("classified description");
            self.a
                .send(Command::UpdateClassified(ClassifiedUpdate {
                    classified_id: classified,
                    category: ClassifiedCategory::Shopping,
                    name,
                    description,
                    price_for_listing: LindenAmount(price),
                    ..ClassifiedUpdate::default()
                }))
                .await?;
            let got = watch(self.a, LISTEN).await?;
            metrics.set(&format!("{leg}_told_a"), self.scene.got_text(&got));
            if let Some(after) = got.money.last().map(|money| money.balance.0) {
                metrics.set(
                    &format!("{leg}_charged"),
                    signed(balance.unwrap_or(after)).saturating_sub(signed(after)),
                );
                balance = Some(after);
            } else {
                metrics.set(&format!("{leg}_charged"), 0_i64);
            }
            self.a
                .send(Command::RequestClassifiedInfo(classified))
                .await?;
            let got = watch_for(self.a, LISTEN, QUIET, |got| {
                got.classified_infos
                    .iter()
                    .any(|info| info.classified_id == classified)
            })
            .await?;
            metrics.set(&format!("{leg}_read_told_a"), self.scene.got_text(&got));
        }

        let Some(paid) = self.classifieds.last().copied() else {
            return Ok(());
        };
        match self.cap(SEARCH_STAT_REQUEST).await {
            Some(url) => {
                let answer = answered(
                    self.http
                        .post(url)
                        .header("Content-Type", "application/llsd+xml")
                        .header("Accept", "application/llsd+xml")
                        .body(llsd_map(vec![("classified_id", Llsd::Uuid(paid.uuid()))])),
                )
                .await?;
                metrics.set("classified_stats_answer", answer.text(&self.scene));
            }
            None => metrics.set("classified_stats_answer", NO_CAPABILITY),
        }

        self.a
            .send(Command::RequestAvatarClassifieds(AgentKey::from(a)))
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.classifieds.iter().any(|(whose, _list)| *whose == a)
        })
        .await?;
        metrics.set("classifieds_listed_told_a", self.scene.got_text(&got));

        let name = self.scene.text("paid classified");
        let description = self.scene.text("classified edited description");
        self.a
            .send(Command::UpdateClassified(ClassifiedUpdate {
                classified_id: paid,
                category: ClassifiedCategory::Shopping,
                name,
                description,
                price_for_listing: LindenAmount(LISTING_PRICE),
                ..ClassifiedUpdate::default()
            }))
            .await?;
        let got = watch(self.a, LISTEN).await?;
        metrics.set("classified_edit_told_a", self.scene.got_text(&got));
        self.a.send(Command::RequestClassifiedInfo(paid)).await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.classified_infos
                .iter()
                .any(|info| info.classified_id == paid)
        })
        .await?;
        metrics.set("classified_edit_read_told_a", self.scene.got_text(&got));

        self.b
            .send(Command::RequestAvatarClassifieds(AgentKey::from(a)))
            .await?;
        let got = watch_for(self.b, LISTEN, QUIET, |got| {
            got.classifieds.iter().any(|(whose, _list)| *whose == a)
        })
        .await?;
        metrics.set("classifieds_other_reads_told_b", self.scene.got_text(&got));

        self.a
            .send(Command::RequestClassifiedClickStats(paid))
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| !got.click_stats.is_empty()).await?;
        metrics.set("client_click_stats_told_a", self.scene.got_text(&got));

        for (leg, classified) in self
            .classifieds
            .clone()
            .into_iter()
            .zip(["classified_free_delete", "classified_paid_delete"])
            .map(|(classified, leg)| (leg, classified))
        {
            self.a.send(Command::DeleteClassified(classified)).await?;
            let got = watch(self.a, SETTLE).await?;
            metrics.set(&format!("{leg}_told_a"), self.scene.got_text(&got));
            self.a
                .send(Command::RequestClassifiedInfo(classified))
                .await?;
            let got = watch_for(self.a, LISTEN, QUIET, |got| {
                got.classified_infos
                    .iter()
                    .any(|info| info.classified_id == classified)
            })
            .await?;
            metrics.set(&format!("{leg}_read_told_a"), self.scene.got_text(&got));
        }
        self.classifieds.clear();
        Ok(())
    }

    /// Leg 9: the profile image.
    async fn profile_image(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let a = self.scene.a;
        let Some(url) = self.cap(UPLOAD_AGENT_PROFILE_IMAGE).await else {
            for key in [
                "image_uploader_answer",
                "image_upload_answer",
                "image_reads_cap",
                "image_reads_udp",
                "image_read_told_a",
                "image_told_a",
            ] {
                metrics.set(key, NO_CAPABILITY);
            }
            return Ok(());
        };
        let first = answered(
            self.http
                .post(url)
                .header("Content-Type", "application/llsd+xml")
                .header("Accept", "application/llsd+xml")
                .body(llsd_map(vec![(
                    "profile-image-asset",
                    Llsd::String("fl_image_id".to_owned()),
                )])),
        )
        .await?;
        metrics.set("image_uploader_answer", first.text(&self.scene));
        let uploader = match first.llsd().as_ref().and_then(|llsd| llsd.get("uploader")) {
            Some(Llsd::String(uploader) | Llsd::Uri(uploader)) => uploader.clone(),
            _ => {
                for key in [
                    "image_upload_answer",
                    "image_reads_cap",
                    "image_reads_udp",
                    "image_read_told_a",
                    "image_told_a",
                ] {
                    metrics.set(key, "no uploader");
                }
                return Ok(());
            }
        };
        let image = profile_image_bytes()?;
        self.changed.push(Change::FirstLifeImage);
        let second = answered(
            self.http
                .post(uploader)
                .header("Content-Type", "application/jp2")
                .header("Accept", "application/llsd+xml")
                .body(image),
        )
        .await?;
        if let Some(Llsd::Uuid(asset)) = second
            .llsd()
            .as_ref()
            .and_then(|llsd| llsd.get("new_asset"))
        {
            self.scene.named.push((*asset, "the uploaded image"));
        }
        metrics.set("image_upload_answer", second.text(&self.scene));
        let told = watch(self.a, SETTLE).await?;
        metrics.set("image_told_a", self.scene.got_text(&told));
        metrics.set("image_reads_cap", self.cap_field(a, "fl_image_id").await?);
        let read = self.udp_read(metrics, "image_read", a).await?;
        metrics.set(
            "image_reads_udp",
            read.properties
                .iter()
                .find(|props| props.avatar_id.uuid() == a)
                .map_or_else(
                    || "no profile".to_owned(),
                    |props| self.scene.id_text(props.fl_image_id.uuid()).to_owned(),
                ),
        );
        Ok(())
    }

    /// Leg 10: the same reads and writes through the client's own commands,
    /// which take the capability where the region grants it.
    async fn client(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let (a, b) = (self.scene.a, self.scene.b);
        self.a
            .send(Command::RequestAvatarProperties(AgentKey::from(b)))
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| got.has_properties(b)).await?;
        metrics.set("client_read_other_told_a", self.scene.got_text(&got));

        if let Some(found) = self.found.clone() {
            let about = self.scene.text("client about");
            self.changed.push(Change::Profile);
            self.a
                .send(Command::UpdateProfile(ProfileUpdate {
                    about_text: about,
                    ..profile_update(&found)
                }))
                .await?;
            let told = watch(self.a, SETTLE).await?;
            metrics.set("client_edit_told_a", self.scene.got_text(&told));
            let read = self.udp_read(metrics, "client_edit_read", a).await?;
            metrics.set("client_edit_reads_udp", self.about_of(&read, a));
            metrics.set(
                "client_edit_reads_cap",
                self.cap_field(a, "sl_about_text").await?,
            );
        } else {
            metrics.set("client_edit_told_a", "no profile to edit");
            metrics.set("client_edit_read_told_a", "no profile to edit");
            metrics.set("client_edit_reads_udp", "no profile to edit");
            metrics.set("client_edit_reads_cap", "no profile to edit");
        }

        let note = self.scene.text("client note");
        self.changed.push(Change::Notes);
        self.a
            .send(Command::UpdateAvatarNotes {
                target_id: AgentKey::from(b),
                notes: note,
            })
            .await?;
        let told = watch(self.a, SETTLE).await?;
        metrics.set("client_notes_told_a", self.scene.got_text(&told));
        self.a
            .send(Command::RequestAvatarNotes(AgentKey::from(b)))
            .await?;
        let got = watch_for(self.a, LISTEN, QUIET, |got| {
            got.notes.iter().any(|(who, _notes)| *who == b)
        })
        .await?;
        metrics.set("client_notes_reads", self.notes_text(&got, b));

        self.changed.push(Change::FirstLifeImage);
        self.a
            .send(Command::UploadProfileImage {
                image: ProfileImage::FirstLife,
                j2c: profile_image_bytes()?,
            })
            .await?;
        let got = watch_for(self.a, CLIENT_UPLOAD, QUIET, |got| !got.images.is_empty()).await?;
        if let Some((_image, Some(asset))) = got.images.first() {
            self.scene
                .named
                .push((asset.uuid(), "the client's uploaded image"));
        }
        metrics.set("client_image_told_a", self.scene.got_text(&got));
        metrics.set(
            "client_image_reads_cap",
            self.cap_field(a, "fl_image_id").await?,
        );
        Ok(())
    }

    /// Puts back what the census changed and deletes what it made; every
    /// step is tried, and the first failure is the answer.
    async fn clean_up(&mut self) -> Result<(), TestFailure> {
        let mut outcome = Ok(());
        let mut keep = |result: Result<(), TestFailure>| {
            if outcome.is_ok() {
                outcome = result;
            }
        };
        for pick in std::mem::take(&mut self.picks) {
            keep(self.a.send(Command::DeletePick(pick)).await);
        }
        for classified in std::mem::take(&mut self.classifieds) {
            keep(self.a.send(Command::DeleteClassified(classified)).await);
        }
        // The profile is put back as the client writes it, over whichever
        // road the region offers: the texts, both images and, where the census
        // set it, the age flag.
        let profile_changed = [Change::Profile, Change::FirstLifeImage, Change::HideAge]
            .iter()
            .any(|change| self.changed.contains(change));
        if profile_changed && let Some(found) = self.found.clone() {
            keep(
                self.a
                    .send(Command::UpdateProfile(ProfileUpdate {
                        hide_age: self
                            .changed
                            .contains(&Change::HideAge)
                            .then(|| self.found_hide_age.unwrap_or(false)),
                        ..profile_update(&found)
                    }))
                    .await,
            );
        }
        if self.changed.contains(&Change::Interests)
            && let Some(found) = self.found_interests.clone()
        {
            keep(
                self.a
                    .send(Command::UpdateInterests(InterestsUpdate {
                        want_to_mask: found.want_to_mask,
                        want_to_text: found.want_to_text,
                        skills_mask: found.skills_mask,
                        skills_text: found.skills_text,
                        languages_text: found.languages_text,
                    }))
                    .await,
            );
        }
        if self.changed.contains(&Change::Notes) {
            keep(
                self.a
                    .send(Command::UpdateAvatarNotes {
                        target_id: AgentKey::from(self.scene.b),
                        notes: self.found_notes.clone().unwrap_or_default(),
                    })
                    .await,
            );
        }
        // Give the grid the writes before the logout.
        tokio::time::sleep(SETTLE).await;
        outcome
    }
}

/// `session`'s agent and session ids, for a message the census builds itself.
fn ids_of(session: &Session) -> Result<(Uuid, Uuid), TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let session_id = session
        .session_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
    Ok((agent.uuid(), session_id))
}

/// Sends `message` over `session`'s circuit as it is, reliably.
async fn send_raw(session: &Session, message: AnyMessage) -> Result<(), TestFailure> {
    session
        .send(Command::Send {
            message: Box::new(message),
            reliability: Reliability::Reliable,
        })
        .await
}

/// `text` NUL-terminated, as the reference packs a string field.
fn nul_terminated(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// A profile update that writes back what `props` reads.
fn profile_update(props: &AvatarProperties) -> ProfileUpdate {
    /// The `AVATAR_ALLOW_PUBLISH` profile flag.
    const ALLOW_PUBLISH: u32 = 0x1;
    /// The `AVATAR_MATURE_PUBLISH` profile flag.
    const MATURE_PUBLISH: u32 = 0x2;
    ProfileUpdate {
        image_id: props.image_id,
        fl_image_id: props.fl_image_id,
        about_text: props.about_text.clone(),
        fl_about_text: props.fl_about_text.clone(),
        allow_publish: props.flags & ALLOW_PUBLISH != 0,
        mature_publish: props.flags & MATURE_PUBLISH != 0,
        profile_url: props.profile_url.clone(),
        hide_age: None,
    }
}

/// A small checker image as JPEG-2000, the profile image the census uploads.
fn profile_image_bytes() -> Result<Vec<u8>, TestFailure> {
    let size = 64_u32;
    let mut pixels = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let odd = (x / 8).wrapping_add(y / 8) % 2 == 1;
            pixels.extend_from_slice(if odd {
                &[0x22, 0x66, 0xAA, 0xFF]
            } else {
                &[0xEE, 0xEE, 0x22, 0xFF]
            });
        }
    }
    sl_test_assets::RgbaImage {
        width: size,
        height: size,
        pixels,
    }
    .j2c()
    .map_err(|error| TestFailure::Assertion(format!("j2c encode failed: {error}")))
}

/// A word no other run uses.
fn random_word() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(6)
        .collect()
}

/// A census of profiles: reads and writes over both roads, interests, notes,
/// picks, classifieds, display names and the profile image.
#[derive(Debug)]
pub struct Profiles;

impl GridTest for Profiles {
    fn name(&self) -> &'static str {
        "profiles"
    }

    fn description(&self) -> &'static str {
        "How profiles, interests, notes, picks, classifieds and display names are read and written"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The census: both avatars in, the legs, and the clean up whatever became of
/// them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let ctx_grid = ctx.grid();
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "profiles needs a secondary avatar".to_owned(),
        ));
    };
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    // The profile replies are low priority: raise the throttle as a viewer
    // does, or a bare login sends none of them.
    a.send(Command::SetThrottle(Throttle::preset_1000()))
        .await?;
    b.send(Command::SetThrottle(Throttle::preset_1000()))
        .await?;
    metrics.set(
        "login_web_profile_url",
        a.login_success()
            .and_then(|login| login.web_profile_url.as_ref())
            .map_or_else(|| "none".to_owned(), |url| url.as_str().to_owned()),
    );
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        a_names: names_of(&a.avatar_legacy_name()),
        b_names: names_of(&b.avatar_legacy_name()),
        tag: format!("{TEXT_TAG}{}", random_word()),
        ..Scene::default()
    };
    let mut census = Census {
        a,
        b,
        scene,
        grid: ctx_grid,
        http: http_client()?,
        found: None,
        found_interests: None,
        found_notes: None,
        found_hide_age: None,
        found_fl_image: None,
        changed: Vec::new(),
        picks: Vec::new(),
        classifieds: Vec::new(),
    };
    let started = Instant::now();
    let outcome = legs(&mut census, metrics).await;
    metrics.set_timing(&secs_metric("census"), started.elapsed().as_secs_f64());
    let cleaned = census.clean_up().await;
    outcome.and(cleaned)
}

/// Every leg, in order.
async fn legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    census.reads(metrics).await?;
    census.udp_edit(metrics).await?;
    census.cap_edits(metrics).await?;
    census.interests(metrics).await?;
    census.notes(metrics).await?;
    census.picks(metrics).await?;
    census.classifieds(metrics).await?;
    census.profile_image(metrics).await?;
    census.client(metrics).await?;
    Ok(())
}
