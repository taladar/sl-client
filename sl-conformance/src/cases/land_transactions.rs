//! A census of land changing hands: buying, selling, passes, deeding,
//! abandoning and reclaiming — what a grid refuses and how it says so, what
//! it charges and tells each side, and what the parcel record reads after.
//!
//! Avatar **A** owns *the plot*; avatar **B** is another resident. On OpenSim
//! A is the estate owner (`--avatar estate-owner --secondary resident`) and
//! the plot is a corner the census divides off the Default Region and joins
//! back afterwards. On Second Life the plot is a parcel A owns, named as
//! `land_parcel` in the fixtures. The test avatars own none on aditi: every
//! parcel for sale there is in a closed Linden region but one, and the
//! grid's land helper refuses every purchase ("Our billing system is
//! currently down", 2026-10-17) — so without one the census stands both
//! avatars on somebody else's parcel at the fixtures' build location and
//! measures the refusals and the helper only.
//!
//! One question a leg:
//!
//! 1. **Refusals**: B buys the plot while it is not for sale, abandons it,
//!    deeds it to a group and reclaims it — none of which B may do — and
//!    buys a pass to it while it names a pass price but keeps no pass list
//!    (Second Life sells one: L$ 10 to the sandbox's group a run). B then
//!    asks the grid's economy helper
//!    (`preflightBuyLandPrep`) what buying the plot would need.
//! 2. **A sale to B**: A sets the plot for sale to B alone at L$ 1; B
//!    offers too little, then buys it at the price.
//! 3. **Back to A**: B sets it for sale to A alone at L$ 1 and A buys it back.
//! 4. **Passes**: A sells passes to the plot; B buys one, then another.
//! 5. **A deed**: A sets the plot's group and deeds the plot to it, reads the
//!    listing's group-owned bit, then sells the group's land to itself for
//!    L$ 0 and buys it.
//! 6. **Abandon and reclaim** (OpenSim only — Second Life's plot is kept): A
//!    sells the plot to B for L$ 0, B abandons it and A, the estate owner,
//!    reclaims it.
//!
//! Every leg records what A and B were told (`*_told_a`, `*_told_b`: alerts,
//! instant messages, balance replies, parcel records and overlays) and what
//! the plot reads after (`*_after`). Every id is described by whose it is and
//! every name is scrubbed, so no metric names an avatar.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/land.md` (§ Transactions); every run is held to it.
//!
//! `2av`, live grids.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Diagnostic, Event, GroupKey, InstantMessage, LindenAmount, MoneyBalance, OwnerKey,
    ParcelAccessEntry, ParcelAccessScope, ParcelDetails, ParcelFlags, ParcelInfo, ParcelKey,
    ParcelStatus, RegionCoordinates, RegionHandle, ScopedParcelId, Uuid, Vector,
};

use crate::cases::asset_http::without_ids;
use crate::cases::groups::names_of;
use crate::cases::object_rez_derez::Answer;
use crate::cases::parcel_divide_join::restore_single_parcel;
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, is_opensim};

/// How long both avatars are listened to after a leg's request.
const WINDOW: Duration = Duration::from_secs(8);

/// How long a reply the census waits on (a parcel read, a listing) may take.
const REPLY: Duration = Duration::from_secs(15);

/// How long a teleport may take.
const TELEPORT: Duration = Duration::from_secs(60);

/// How long the census leaves a listing unread before reading the deed into
/// it: OpenSim caches a listing for 30 s past its last read.
const LISTING_REST: Duration = Duration::from_secs(45);

/// The price of every sale and pass the census makes, in L$.
const PRICE: i32 = 1;

/// How many hours a pass lasts.
const PASS_HOURS: f32 = 1.0;

/// The corner of OpenSim's Default Region the census divides off as its plot,
/// `(west, south, east, north)`.
const OPENSIM_PLOT: (f32, f32, f32, f32) = (0.0, 0.0, 32.0, 32.0);

/// The legs only an owner of the plot can run, in order.
const OWNER_LEGS: [&str; 18] = [
    "sale_to_b",
    "buy_short",
    "buy",
    "sale_to_a",
    "buy_back",
    "pass_set",
    "pass_buy",
    "pass_again",
    "pass_list",
    "pass_clear",
    "group_set",
    "deed",
    "group_sale_to_a",
    "group_buy_back",
    "free_sale_to_b",
    "free_buy",
    "release",
    "reclaim",
];

/// The legs that give the plot away for good, run on OpenSim only: Second
/// Life's plot is the fixtures' and is kept.
const KEPT_PLOT_LEGS: [&str; 4] = ["free_sale_to_b", "free_buy", "release", "reclaim"];

/// The first sequence id of the census's own parcel reads.
const SEQUENCE_BASE: i32 = 7100;

/// The language the helper calls state.
const LANGUAGE: &str = "en";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/land.md (land-transactions, 2026-10-17)";

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
/// Not held: whether the plot and the group came from the fixtures.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("b_active_group", both(Answer::Text("none"))),
    (
        "buy_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by B, group no group, not for sale (price 0), leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "buy_back_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "buy_back_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [], pass L$ 1 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "buy_back_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [], pass L$ 1 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "buy_short_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group no group, for sale at L$ 1 to B, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "buy_short_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_short_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by B, group no group, not for sale (price 0), leased, [], pass L$ 1 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "buy_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by B, group no group, not for sale (price 0), leased, [], pass L$ 1 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "deed_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by the group (group), group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "deed_listing",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("flags 0x04, owner the group"),
        ),
    ),
    (
        "deed_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by the group (group), group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "deed_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by the group (group), group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "free_buy_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by B, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "free_buy_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by B, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "free_buy_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by B, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "free_sale_to_b_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group no group, for sale at L$ 0 to B, leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "free_sale_to_b_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, for sale at L$ 0 to B, leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "free_sale_to_b_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, for sale at L$ 0 to B, leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_buy_back_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_buy_back_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "group_buy_back_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "group_sale_to_a_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by the group (group), group the group, for sale at L$ 0 to A, leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_sale_to_a_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by the group (group), group the group, for sale at L$ 0 to A, leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_sale_to_a_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by the group (group), group the group, for sale at L$ 0 to A, leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_set_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_set_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "group_set_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "pass_again_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [access list, passes], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "pass_again_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pass_again_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("agent alert \"/You already have a pass valid for <n> minutes\""),
        ),
    ),
    (
        "pass_buy_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [access list, passes], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "pass_buy_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pass_buy_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pass_clear_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "pass_clear_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "pass_clear_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "pass_list_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [access list, passes], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "pass_list_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("Access list: [B flags 1 timed]"),
        ),
    ),
    (
        "pass_list_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pass_set_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group the group, not for sale (price 0), leased, [access list, passes], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "pass_set_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [access list, passes], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "pass_set_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group the group, not for sale (price 0), leased, [access list, passes], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "pass_unlisted_after",
        each(
            Answer::Text(
                "owned by another (group), group another, not for sale (price 0), leased, [in search], pass L$ 10 for 10 h",
            ),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    ("pass_unlisted_told_a", both(Answer::Text("nothing"))),
    (
        "pass_unlisted_told_b",
        each(
            Answer::Text(
                "alert \"You purchased a pass.\"; balance type 5006 L$ 10 from B to another \"You paid Public Sandbox Group  L$10 for a land access pass.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "plot_start",
        each(
            Answer::Text(
                "owned by another (group), group another, not for sale (price 0), leased, [in search], pass L$ 10 for 10 h",
            ),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 0 h",
            ),
        ),
    ),
    (
        "preflight_b",
        each(
            Answer::Text(
                "via Second Life's helper: HTTP 200, failure \"Our billing system is currently down.  Please try again later.\"; keys errorMessage success",
            ),
            Answer::Text(
                "via the region's currency-base-uri: HTTP 200, success, membership upgrade false, land use upgrade false, estimate in cents, confirm a token; keys confirm currency.estimatedCost landuse.action landuse.upgrade membership{} success",
            ),
        ),
    ),
    (
        "preflight_b_billable",
        each(Answer::Text("the area"), Answer::Text("none")),
    ),
    (
        "reclaim_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "reclaim_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "reclaim_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "refuse_buy_not_for_sale_after",
        each(
            Answer::Text(
                "owned by another (group), group another, not for sale (price 0), leased, [in search], pass L$ 10 for 10 h",
            ),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 0 h",
            ),
        ),
    ),
    (
        "refuse_buy_not_for_sale_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_buy_not_for_sale_told_b",
        each(
            Answer::Text("alert \"Unable to buy, this parcel is not for sale.\"; 4 overlay chunks"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "refuse_deed_after",
        each(
            Answer::Text(
                "owned by another (group), group another, not for sale (price 0), leased, [in search], pass L$ 10 for 10 h",
            ),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 0 h",
            ),
        ),
    ),
    ("refuse_deed_told_a", both(Answer::Text("nothing"))),
    (
        "refuse_deed_told_b",
        each(
            Answer::Text("alert \"Unable to deed parcel to group.\""),
            Answer::Text("nothing"),
        ),
    ),
    (
        "refuse_reclaim_after",
        each(
            Answer::Text(
                "owned by another (group), group another, not for sale (price 0), leased, [in search], pass L$ 10 for 10 h",
            ),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 0 h",
            ),
        ),
    ),
    ("refuse_reclaim_told_a", both(Answer::Text("nothing"))),
    ("refuse_reclaim_told_b", both(Answer::Text("nothing"))),
    (
        "refuse_release_after",
        each(
            Answer::Text(
                "owned by another (group), group another, not for sale (price 0), leased, [in search], pass L$ 10 for 10 h",
            ),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 0 h",
            ),
        ),
    ),
    ("refuse_release_told_a", both(Answer::Text("nothing"))),
    ("refuse_release_told_b", both(Answer::Text("nothing"))),
    (
        "release_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h",
            ),
        ),
    ),
    (
        "release_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "release_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, not for sale (price 0), leased, [], pass L$ 0 for 1 h; 4 overlay chunks",
            ),
        ),
    ),
    (
        "sale_to_a_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by B, group no group, for sale at L$ 1 to A, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "sale_to_a_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by B, group no group, for sale at L$ 1 to A, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "sale_to_a_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by B, group no group, for sale at L$ 1 to A, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "sale_to_b_after",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "owned by A, group no group, for sale at L$ 1 to B, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "sale_to_b_told_a",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, for sale at L$ 1 to B, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
    (
        "sale_to_b_told_b",
        each(
            Answer::Text("not measured: A owns no land here (gridspec-aditi-test-land)"),
            Answer::Text(
                "record -10000 of the plot: owned by A, group no group, for sale at L$ 1 to B, leased, [], pass L$ 1 for 1 h",
            ),
        ),
    ),
];

/// Buying, selling, passes, deeding, abandoning and reclaiming land.
#[derive(Debug)]
pub struct LandTransactions;

impl GridTest for LandTransactions {
    fn name(&self) -> &'static str {
        "land-transactions"
    }

    fn description(&self) -> &'static str {
        "Buying, selling, passes, deeding, abandoning and reclaiming land: refusals, charges, \
         what each side is told and what the parcel reads after"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
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

/// Which avatar acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The plot's owner.
    A,
    /// The other resident.
    B,
}

/// Who is who, for describing ids and scrubbing names.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// The group the plot is deeded to.
    group: Uuid,
    /// The region's estate owner, when it is somebody else.
    estate_owner: Uuid,
    /// Every way a grid may write A's or B's name, with the word that
    /// replaces it.
    names: Vec<(String, &'static str)>,
}

impl Scene {
    /// Whose `id` is, in a word.
    fn whose(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "A"
        } else if id == self.b {
            "B"
        } else if id == self.group {
            "the group"
        } else if id == self.estate_owner {
            "the estate owner"
        } else {
            "another"
        }
    }

    /// `text` with every id and every name of A's and B's replaced.
    fn scrub(&self, text: &str) -> String {
        let mut out = without_ids(text);
        for (name, word) in &self.names {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        out
    }
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every alert: which message carried it, and its text.
    alerts: Vec<(&'static str, String)>,
    /// Every instant message.
    ims: Vec<InstantMessage>,
    /// Every balance reply.
    money: Vec<MoneyBalance>,
    /// Every parcel record, pushed or answered.
    parcels: Vec<ParcelInfo>,
    /// How many overlay chunks arrived.
    overlays: usize,
    /// Every access-list reply.
    access: Vec<(ParcelAccessScope, Vec<ParcelAccessEntry>)>,
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event: &Event) -> Option<()> {
    match event {
        Event::AlertMessage { message, .. } => got.alerts.push(("alert", message.clone())),
        Event::AgentAlertMessage { message, modal, .. } => got.alerts.push((
            if *modal {
                "modal agent alert"
            } else {
                "agent alert"
            },
            message.clone(),
        )),
        Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
        Event::MoneyBalance(balance) => got.money.push(balance.clone()),
        Event::ParcelProperties(parcel) => got.parcels.push((**parcel).clone()),
        Event::ParcelOverlay(_) => got.overlays = got.overlays.saturating_add(1),
        Event::ParcelAccessList { scope, entries, .. } => {
            got.access.push((*scope, entries.clone()));
        }
        _ => return None,
    }
    Some(())
}

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
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

/// The names of the event-queue messages `session` was sent and did not read
/// since it had seen `seen` diagnostics, and the new count.
fn unread_since(session: &Session, seen: usize) -> (Vec<String>, usize) {
    let all = session.diagnostics();
    let names = all
        .iter()
        .skip(seen)
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::UnknownCapsEvent { message } => Some(message.clone()),
            _ => None,
        })
        .collect();
    (names, all.len())
}

/// A parcel record in words: whose it is, its group, whether it is for sale
/// and to whom, its status and its passes.
fn describe_parcel(scene: &Scene, parcel: &ParcelInfo) -> String {
    let owner = match parcel.owner {
        OwnerKey::Group(group) => format!("owned by {} (group)", scene.whose(group.uuid())),
        OwnerKey::Agent(agent) => format!("owned by {}", scene.whose(agent.uuid())),
    };
    let group = parcel
        .group
        .map_or("no group", |group| scene.whose(group.uuid()));
    let flags = parcel.flags();
    let sale = if flags.contains(ParcelFlags::FOR_SALE) {
        format!(
            "for sale at L$ {} to {}",
            parcel
                .sale_price
                .as_ref()
                .map_or_else(|| "?".to_owned(), |price| price.0.to_string()),
            parcel
                .auth_buyer_id
                .map_or("anyone", |buyer| scene.whose(buyer.uuid()))
        )
    } else {
        let price = parcel.sale_price.as_ref().map_or(0, |price| price.0);
        format!("not for sale (price {price})")
    };
    let status = match parcel.status {
        ParcelStatus::Leased => "leased".to_owned(),
        ParcelStatus::LeasePending => "lease pending".to_owned(),
        ParcelStatus::Abandoned => "abandoned".to_owned(),
        ParcelStatus::None => "no status".to_owned(),
        ParcelStatus::Unknown(value) => format!("status {value}"),
        other => format!("{other:?}"),
    };
    let mut access = Vec::new();
    for (flag, word) in [
        (ParcelFlags::USE_ACCESS_LIST, "access list"),
        (ParcelFlags::USE_ACCESS_GROUP, "group access"),
        (ParcelFlags::USE_PASS_LIST, "passes"),
        (ParcelFlags::SHOW_DIRECTORY, "in search"),
    ] {
        if flags.contains(flag) {
            access.push(word);
        }
    }
    let passes = format!(
        "pass L$ {} for {} h",
        parcel.pass_price.0, parcel.pass_hours
    );
    format!(
        "{owner}, group {group}, {sale}, {status}, [{}], {passes}",
        access.join(", ")
    )
}

/// A balance reply in words: whether it succeeded, the transaction it names
/// and its description — never the balance itself.
fn describe_money(scene: &Scene, money: &MoneyBalance) -> String {
    let transaction = money.transaction.as_ref().map_or_else(
        || "no transaction".to_owned(),
        |transaction| {
            format!(
                "type {} L$ {} from {} to {}",
                transaction.transaction_type,
                transaction.amount.0,
                scene.whose(transaction.source.uuid()),
                scene.whose(transaction.dest.uuid())
            )
        },
    );
    format!(
        "balance{} {transaction} \"{}\"",
        if money.success { "" } else { " (failed)" },
        scene.scrub(&money.description)
    )
}

/// Everything `got` holds about the plot `plot`, in words; `nothing` for an
/// empty window.
fn describe_got(scene: &Scene, got: &Got, plot: Option<i32>, unread: &[String]) -> String {
    let mut parts = Vec::new();
    for (kind, alert) in &got.alerts {
        parts.push(format!(
            "{kind} \"{}\"",
            without_figures(&scene.scrub(alert))
        ));
    }
    for im in &got.ims {
        parts.push(format!(
            "im {:?} from {} \"{}\"",
            im.dialog,
            scene.whose(im.from_agent_id.uuid()),
            scene.scrub(&im.message)
        ));
    }
    for money in &got.money {
        parts.push(describe_money(scene, money));
    }
    for parcel in &got.parcels {
        let which = if Some(parcel.local_id.0) == plot {
            "the plot"
        } else {
            "another parcel"
        };
        parts.push(format!(
            "record {} of {which}: {}",
            parcel.sequence_id,
            describe_parcel(scene, parcel)
        ));
    }
    if got.overlays > 0 {
        parts.push(format!("{} overlay chunks", got.overlays));
    }
    for (scope, entries) in &got.access {
        parts.push(format!(
            "{scope:?} list: [{}]",
            entries
                .iter()
                .map(|entry| format!(
                    "{} flags {} {}",
                    scene.whose(entry.id),
                    entry.flags.0,
                    if entry.time == 0 { "forever" } else { "timed" }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for name in unread {
        parts.push(format!("unread event {name}"));
    }
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("; ")
    }
}

/// `price` L$ as an amount (a negative price is none).
fn amount(price: i32) -> LindenAmount {
    LindenAmount(u64::try_from(price).unwrap_or(0))
}

/// An amount as the `S32` price a `ParcelBuy` carries (saturating).
fn price_of(amount: &LindenAmount) -> i32 {
    i32::try_from(amount.0).unwrap_or(i32::MAX)
}

/// The area the helper is asked about: the region's billable factor times
/// the parcel's area, rounded, as the reference asks it.
fn billable_area(session: &Session, parcel: &ParcelInfo) -> Result<i32, TestFailure> {
    let factor = session
        .billable_factor()
        .ok_or_else(|| TestFailure::State("no region handshake was read".to_owned()))?;
    let area = f64::from(parcel.area.0) * f64::from(factor);
    Ok(format!("{:.0}", area.round()).parse().unwrap_or(i32::MAX))
}

/// A parcel's area as the `S32` a `ParcelBuy` and the helper carry.
fn area_of(parcel: &ParcelInfo) -> i32 {
    i32::try_from(parcel.area.0).unwrap_or(i32::MAX)
}

/// `text` with every decimal figure (`59.87`) replaced by `<n>`: a time
/// left that differs from run to run.
fn without_figures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.next() {
        if !first.is_ascii_digit() {
            out.push(first);
            continue;
        }
        let mut run = String::from(first);
        while let Some(next) = chars.next_if(|next| next.is_ascii_digit() || *next == '.') {
            run.push(next);
        }
        let decimal = run
            .split_once('.')
            .is_some_and(|(_whole, part)| !part.is_empty());
        if decimal {
            out.push_str("<n>");
        } else {
            out.push_str(&run);
        }
    }
    out
}

/// The plot: where it is and what it was when the census found it.
#[derive(Debug, Clone)]
struct Plot {
    /// Its region-local id.
    local_id: i32,
    /// A point inside it, region-local.
    point: Vector,
    /// Its area in m².
    area: i32,
    /// Its grid-wide id, once known.
    parcel_id: Option<ParcelKey>,
    /// Whether A owns it: on Second Life without land of A's, the census
    /// measures only the refusals, on somebody else's parcel.
    owned: bool,
}

/// The census's state: the two sessions, who is who and the plot.
struct Census<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Who is who.
    scene: Scene,
    /// The plot.
    plot: Plot,
    /// The next sequence id of a parcel read.
    sequence: i32,
    /// How many diagnostics each session had shown when last looked at.
    seen: (usize, usize),
    /// The grid.
    grid: Grid,
}

impl Census<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// The plot, scoped to `who`'s circuit.
    fn plot_for(&self, who: Who) -> Result<ScopedParcelId, TestFailure> {
        let session = match who {
            Who::A => &*self.a,
            Who::B => &*self.b,
        };
        let circuit = session
            .circuit_id()
            .ok_or_else(|| TestFailure::State("no root circuit".to_owned()))?;
        Ok(ScopedParcelId::new(
            circuit,
            sl_client_tokio::RegionLocalParcelId(self.plot.local_id),
        ))
    }

    /// Lets both avatars hear out what the grid still had to say — an
    /// arrival's records, a setup's pushes — so it is not taken for the
    /// next leg's answer.
    async fn settle(&mut self) -> Result<(), TestFailure> {
        let (got_a, got_b) = tokio::join!(watch(self.a, WINDOW), watch(self.b, WINDOW));
        let _heard: (Got, Got) = (got_a?, got_b?);
        Ok(())
    }

    /// Reads the plot as `who` sees it.
    async fn read(&mut self, who: Who) -> Result<ParcelInfo, TestFailure> {
        self.sequence = self.sequence.saturating_add(1);
        let sequence_id = self.sequence;
        let point = self.plot.point.clone();
        read_parcel(self.session(who), &point, sequence_id).await
    }

    /// Sends `commands` as `who`, listens to both avatars, and records what
    /// each was told under `key` and the plot as A reads it after.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<(Got, Got, ParcelInfo), TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        let (got_a, got_b) = tokio::join!(watch(self.a, WINDOW), watch(self.b, WINDOW));
        let (got_a, got_b) = (got_a?, got_b?);
        let (unread_a, seen_a) = unread_since(self.a, self.seen.0);
        let (unread_b, seen_b) = unread_since(self.b, self.seen.1);
        self.seen = (seen_a, seen_b);
        let plot = Some(self.plot.local_id);
        metrics.set(
            &format!("{key}_told_a"),
            describe_got(&self.scene, &got_a, plot, &unread_a),
        );
        metrics.set(
            &format!("{key}_told_b"),
            describe_got(&self.scene, &got_b, plot, &unread_b),
        );
        let after = self.read(Who::A).await?;
        metrics.set(
            &format!("{key}_after"),
            describe_parcel(&self.scene, &after),
        );
        Ok((got_a, got_b, after))
    }

    /// Puts the plot up for sale to `buyer` at `price`, as its owner `seller`.
    fn sale(&self, record: &ParcelInfo, price: i32, buyer: Uuid) -> Command {
        let mut update = record.to_update();
        update.parcel_flags = update.parcel_flags.union(ParcelFlags::FOR_SALE);
        update.sale_price = Some(amount(price));
        update.auth_buyer_id = Some(sl_client_tokio::AgentKey::from(buyer));
        let _plot = self.plot.local_id;
        Command::UpdateParcel(Box::new(update))
    }

    /// A purchase of the plot by `who` at `price`, carrying the group the
    /// reference's buy carries (the buyer's active one).
    fn buy(&self, who: Who, price: i32, group: Option<GroupKey>) -> Result<Command, TestFailure> {
        Ok(Command::BuyParcel {
            local_id: self.plot_for(who)?,
            price,
            area: self.plot.area,
            group_id: group,
            is_group_owned: false,
        })
    }
}

/// Reads the parcel under the 4 m square containing `point`.
async fn read_parcel(
    session: &mut Session,
    point: &Vector,
    sequence_id: i32,
) -> Result<ParcelInfo, TestFailure> {
    let west = (point.x / 4.0).floor() * 4.0;
    let south = (point.y / 4.0).floor() * 4.0;
    session
        .send(Command::RequestParcelProperties {
            west,
            south,
            east: west + 4.0,
            north: south + 4.0,
            sequence_id,
            snap_selection: false,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// The group `session` has active, from a fresh agent-data update.
async fn active_group(session: &mut Session) -> Result<Option<GroupKey>, TestFailure> {
    session.send(Command::RequestAgentDataUpdate).await?;
    session
        .wait_for(REPLY, |event| match event {
            Event::ActiveGroupChanged(active) => Some(active.active_group_id),
            _ => None,
        })
        .await
}

/// The listing of `parcel_id`.
async fn listing(
    session: &mut Session,
    parcel_id: ParcelKey,
) -> Result<ParcelDetails, TestFailure> {
    session
        .send(Command::RequestParcelInfo { parcel_id })
        .await?;
    session
        .wait_for(REPLY, |event| match event {
            Event::ParcelDetails(details) if details.parcel_id == parcel_id => {
                Some(details.clone())
            }
            _ => None,
        })
        .await
}

/// The grid-wide id of the parcel at `point` in the region `session` stands
/// in.
async fn parcel_id_at(session: &mut Session, point: &Vector) -> Result<ParcelKey, TestFailure> {
    let region_handle = session
        .region_handle()
        .ok_or_else(|| TestFailure::State("no region handle".to_owned()))?;
    let location = RegionCoordinates::new(point.x, point.y, point.z);
    session
        .send(Command::RequestRemoteParcelId {
            location,
            region_id: Uuid::nil(),
            region_handle,
        })
        .await?;
    session
        .wait_for(REPLY, |event| match event {
            Event::RemoteParcelId {
                parcel_id,
                location: asked,
                ..
            } if *asked == location => Some(*parcel_id),
            _ => None,
        })
        .await
}

/// Teleports `session` to the region-local `point` of the region
/// `region_handle`, failing with the grid's reason; the arrival's handshake
/// is read, so the session knows the region's billable factor after.
async fn teleport(
    session: &mut Session,
    region_handle: RegionHandle,
    point: &Vector,
) -> Result<(), TestFailure> {
    if session.region_handle() == Some(region_handle) {
        return Ok(());
    }
    session
        .send(Command::Teleport {
            region_handle,
            position: RegionCoordinates::new(point.x, point.y, point.z + 2.0),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let mut arrived = false;
    let mut shaken = false;
    let started = Instant::now();
    while !(arrived && shaken) {
        let remaining = TELEPORT.saturating_sub(started.elapsed());
        let step = session
            .wait_for(remaining, |event| match event {
                Event::RegionChanged { .. } => Some(Ok(true)),
                Event::RegionInfoHandshake(_) => Some(Ok(false)),
                Event::TeleportFailed { reason, .. } => Some(Err(reason.clone())),
                _ => None,
            })
            .await?;
        match step {
            Ok(true) => arrived = true,
            Ok(false) => shaken = true,
            Err(reason) => {
                return Err(TestFailure::State(format!("the teleport failed: {reason}")));
            }
        }
    }
    Ok(())
}

/// Where a listing's parcel is: its region's handle and a region-local
/// point.
fn place_of(details: &ParcelDetails) -> Result<(RegionHandle, Vector), TestFailure> {
    let (grid, local) = details
        .global_position
        .split()
        .ok_or_else(|| TestFailure::State("the listing names no place".to_owned()))?;
    Ok((
        RegionHandle::from(grid),
        Vector {
            x: local.x(),
            y: local.y(),
            z: local.z(),
        },
    ))
}

/// The economy helper's answer to a `preflightBuyLandPrep`, in words, and the
/// decoded answer when it is one.
async fn preflight(
    session: &Session,
    billable_area: i32,
) -> Result<(String, Option<sl_wire::LandPrep>), TestFailure> {
    let login_uri = session
        .login_uri()
        .ok_or_else(|| TestFailure::State("the avatar has no login URI".to_owned()))?;
    let region_base = region_currency_base(session).await;
    let Some((base, source)) = sl_wire::economy_helper_uri(&login_uri, region_base.as_ref(), None)
    else {
        return Ok(("no helper".to_owned(), None));
    };
    let road = match source {
        sl_wire::HelperSource::RegionFeatures => "the region's currency-base-uri",
        sl_wire::HelperSource::GridInfo => "the grid info",
        sl_wire::HelperSource::SecondLife => "Second Life's helper",
    };
    let login = session
        .login_success()
        .ok_or_else(|| TestFailure::State("no login response".to_owned()))?;
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::State("no agent id".to_owned()))?;
    let request = sl_wire::LandPrepRequest {
        agent_id: agent.uuid(),
        secure_session_id: login.secure_session_id,
        language: LANGUAGE.to_owned(),
        billable_area,
        currency_buy: 0,
        level_id: None,
        estimated_cost: None,
        estimated_local_cost: None,
        confirm: None,
        password: None,
    };
    let url = sl_wire::helper_script_url(&base, sl_wire::LAND_TOOL_HELPER_PATH)
        .map_err(|error| TestFailure::State(error.to_string()))?;
    let answer = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "text/xml")
        .body(sl_wire::build_preflight_land_prep_request(&request))
        .send()
        .await;
    let response = match answer {
        Ok(response) => response,
        Err(error) => return Ok((format!("via {road}: unreachable ({error})"), None)),
    };
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    let shape = raw_shape(&body);
    let decoded = sl_wire::parse_preflight_land_prep_response(&body).ok();
    let words = match &decoded {
        Some(sl_wire::HelperOutcome::Ok(prep)) => format!(
            "via {road}: HTTP {status}, success, membership upgrade {}, land use upgrade {}, \
             estimate {}, confirm {}; keys {shape}",
            prep.membership.upgrade,
            prep.land_use.upgrade,
            match (&prep.estimated_cost, &prep.estimated_local_cost) {
                (Some(_), Some(_)) => "in cents and local",
                (Some(_), None) => "in cents",
                (None, Some(_)) => "local",
                (None, None) => "none",
            },
            if prep.confirm.is_empty() {
                "none"
            } else if prep.confirm == "password" {
                "password"
            } else {
                "a token"
            }
        ),
        Some(sl_wire::HelperOutcome::Failed(failure)) => format!(
            "via {road}: HTTP {status}, failure \"{}\"; keys {shape}",
            without_ids(&failure.error_message)
        ),
        None => format!(
            "via {road}: HTTP {status}, not an answer \"{}\"",
            without_ids(&body.chars().take(60).collect::<String>())
        ),
    };
    let prep = match decoded {
        Some(sl_wire::HelperOutcome::Ok(prep)) => Some(prep),
        _ => None,
    };
    Ok((words, prep))
}

/// The member names of an XML-RPC answer's struct, nested ones dotted, in
/// order — so a grid's spelling of a key is on the record.
fn raw_shape(body: &str) -> String {
    let Ok(response) = sl_wire::xmlrpc::parse_method_response(body) else {
        return "unparsable".to_owned();
    };
    let Some(first) = response.first_param() else {
        return "fault".to_owned();
    };
    let mut keys = Vec::new();
    collect_keys(first, "", &mut keys);
    keys.sort();
    keys.join(" ")
}

/// Collects the dotted member names of `value` under `prefix`.
fn collect_keys(value: &sl_wire::Llsd, prefix: &str, keys: &mut Vec<String>) {
    if let sl_wire::Llsd::Map(map) = value {
        for (key, inner) in map {
            let name = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if let sl_wire::Llsd::Map(nested) = inner {
                if nested.is_empty() {
                    keys.push(format!("{name}{{}}"));
                }
                collect_keys(inner, &name, keys);
            } else {
                keys.push(name);
            }
        }
    }
}

/// The region's `currency-base-uri`, if its `SimulatorFeatures` names one.
async fn region_currency_base(session: &Session) -> Option<url::Url> {
    let url = session.cap("SimulatorFeatures")?;
    let body = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let llsd = sl_wire::parse_llsd_xml(&body).ok()?;
    let features = sl_wire::parse_simulator_features(&llsd).ok()?;
    features.open_sim_extras?.currency_base_uri
}

/// The census: both avatars at the plot, the legs, and the clean-up whatever
/// became of them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let land_parcel = ctx.land_parcel();
    let land_group = ctx.land_group();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "land-transactions needs a secondary avatar".to_owned(),
        ));
    };
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for name in names_of(&a.avatar_legacy_name()) {
        names.push((name, "<A>"));
    }
    for name in names_of(&b.avatar_legacy_name()) {
        names.push((name, "<B>"));
    }
    // The longest first, so a legacy name is not half-replaced by its first
    // name.
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    let scene = Scene {
        a: a_id,
        b: b_id,
        names,
        ..Scene::default()
    };
    let plot = if is_opensim(grid) {
        opensim_plot(a, b, a_id).await?
    } else {
        second_life_plot(a, b, a_id, land_parcel, somewhere, metrics).await?
    };
    let group = the_group(a, land_group, metrics).await?;
    let mut census = Census {
        a,
        b,
        scene: Scene {
            group: group.uuid(),
            ..scene
        },
        plot,
        sequence: SEQUENCE_BASE,
        seen: (0, 0),
        grid,
    };
    census.settle().await?;
    census.seen = (census.a.diagnostics().len(), census.b.diagnostics().len());
    let owner = census.read(Who::A).await?;
    if let OwnerKey::Agent(agent) = owner.owner
        && agent.uuid() != a_id
    {
        census.scene.estate_owner = agent.uuid();
    }
    metrics.set("plot_start", describe_parcel(&census.scene, &owner));
    let outcome = legs(&mut census, metrics, group).await;
    let cleaned = clean_up(&mut census).await;
    outcome.and(cleaned)
}

/// The plot on OpenSim: the region joined back into one parcel, then a
/// corner divided off it.
async fn opensim_plot(a: &mut Session, b: &mut Session, a_id: Uuid) -> Result<Plot, TestFailure> {
    let (west, south, east, north) = OPENSIM_PLOT;
    let point = Vector {
        x: f32::midpoint(west, east),
        y: f32::midpoint(south, north),
        z: 25.0,
    };
    // An earlier run may have left the corner with somebody else: take it
    // back first, or the join refuses it.
    let leftover = read_parcel(a, &point, SEQUENCE_BASE).await?;
    if leftover.owner.uuid() != a_id {
        let circuit = a
            .circuit_id()
            .ok_or_else(|| TestFailure::State("no root circuit".to_owned()))?;
        a.send(Command::ReclaimParcel {
            local_id: ScopedParcelId::new(circuit, leftover.local_id),
        })
        .await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    restore_single_parcel(a).await?;
    a.send(Command::DivideParcel {
        west,
        south,
        east,
        north,
    })
    .await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let plot = read_parcel(a, &point, SEQUENCE_BASE.saturating_add(1)).await?;
    let rest = read_parcel(
        a,
        &Vector {
            x: 128.0,
            y: 128.0,
            z: 25.0,
        },
        SEQUENCE_BASE.saturating_add(2),
    )
    .await?;
    if plot.local_id == rest.local_id {
        return Err(TestFailure::State(
            "the divide made no plot in the corner".to_owned(),
        ));
    }
    let _drained: Got = watch(b, Duration::from_millis(200)).await?;
    Ok(Plot {
        local_id: plot.local_id.0,
        point,
        area: area_of(&plot),
        parcel_id: None,
        owned: true,
    })
}

/// The plot on Second Life: the fixtures' `land_parcel`, which A owns — or,
/// with none named, the parcel at the fixtures' build location, which
/// neither avatar owns and only the refusals are measured on. Both avatars
/// are taken to it.
async fn second_life_plot(
    a: &mut Session,
    b: &mut Session,
    a_id: Uuid,
    named: Option<ParcelKey>,
    somewhere: Option<(String, Vector)>,
    metrics: &mut Metrics,
) -> Result<Plot, TestFailure> {
    let (parcel_id, place, owned) = if let Some(parcel_id) = named {
        let details = listing(a, parcel_id).await?;
        (Some(parcel_id), place_of(&details)?, true)
    } else {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State(
                "land-transactions needs a land_parcel or a build_location in the fixtures"
                    .to_owned(),
            )
        })?;
        (None, (region_named(a, &region).await?, point), false)
    };
    teleport(a, place.0, &place.1).await?;
    teleport(b, place.0, &place.1).await?;
    let record = read_parcel(a, &place.1, SEQUENCE_BASE).await?;
    if owned && record.owner.uuid() != a_id {
        return Err(TestFailure::State(
            "the fixtures' land_parcel is not A's".to_owned(),
        ));
    }
    metrics.set(
        "plot_is",
        if owned {
            "the fixtures' land_parcel"
        } else {
            "somebody else's parcel at the build location"
        },
    );
    Ok(Plot {
        local_id: record.local_id.0,
        point: place.1,
        area: area_of(&record),
        parcel_id,
        owned,
    })
}

/// The handle of the region called `name`, from the world map.
async fn region_named(session: &mut Session, name: &str) -> Result<RegionHandle, TestFailure> {
    session
        .send(Command::RequestMapByName {
            name: name.to_owned(),
        })
        .await?;
    session
        .wait_for(REPLY, |event| match event {
            Event::MapBlock(block)
                if block
                    .name
                    .as_ref()
                    .is_some_and(|known| known.to_string().eq_ignore_ascii_case(name)) =>
            {
                Some(block.region_handle)
            }
            _ => None,
        })
        .await
}

/// The group the plot is deeded to: the fixtures', else one A founds.
async fn the_group(
    a: &mut Session,
    named: Option<GroupKey>,
    metrics: &mut Metrics,
) -> Result<GroupKey, TestFailure> {
    if let Some(group) = named {
        metrics.set("group_is", "the fixtures'");
        return Ok(group);
    }
    a.send(Command::CreateGroup(sl_client_tokio::CreateGroupParams {
        name: format!(
            "gs land {}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(6)
                .collect::<String>()
        ),
        charter: "land-transactions census".to_owned(),
        show_in_list: false,
        insignia_id: None,
        membership_fee: LindenAmount(0),
        open_enrollment: false,
        allow_publish: false,
        mature_publish: false,
    }))
    .await?;
    let founded = a
        .wait_for(REPLY, |event| match event {
            Event::CreateGroupResult {
                group_id, success, ..
            } => Some(success.then_some(*group_id)),
            _ => None,
        })
        .await?
        .ok_or_else(|| TestFailure::State("A could not found a group".to_owned()))?;
    tracing::warn!(
        "land-transactions founded its group {}: name it as land_group in the fixtures",
        founded.uuid()
    );
    metrics.set("group_is", "founded by this run");
    Ok(founded)
}

/// Every leg, in order.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    group: GroupKey,
) -> Result<(), TestFailure> {
    let b_group = active_group(census.b).await?;
    metrics.set(
        "b_active_group",
        b_group.map_or("none", |group| census.scene.whose(group.uuid())),
    );
    let start = census.read(Who::A).await?;
    let sale_price = start.sale_price.as_ref().map_or(0, price_of);

    // 1. Refusals.
    let buy = census.buy(Who::B, sale_price, b_group)?;
    census
        .leg(metrics, "refuse_buy_not_for_sale", Who::B, vec![buy])
        .await?;
    let plot = census.plot_for(Who::B)?;
    census
        .leg(
            metrics,
            "refuse_release",
            Who::B,
            vec![Command::ReleaseParcel { local_id: plot }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_deed",
            Who::B,
            vec![Command::DeedParcelToGroup {
                local_id: plot,
                group_id: group,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_reclaim",
            Who::B,
            vec![Command::ReclaimParcel { local_id: plot }],
        )
        .await?;
    // A pass to land that names a price but keeps no pass list: Second
    // Life's sandbox names one, and on OpenSim A gives the plot one first.
    if census.plot.owned {
        let record = census.read(Who::A).await?;
        let mut update = record.to_update();
        update.parcel_flags = update.parcel_flags.difference(ParcelFlags::USE_PASS_LIST);
        update.pass_price = amount(PRICE);
        update.pass_hours = PASS_HOURS;
        census
            .a
            .send(Command::UpdateParcel(Box::new(update)))
            .await?;
        census.settle().await?;
    }
    census
        .leg(
            metrics,
            "pass_unlisted",
            Who::B,
            vec![Command::BuyParcelPass { local_id: plot }],
        )
        .await?;

    // What the economy helper says B's purchase of the plot needs.
    let record = census.read(Who::B).await?;
    let billable = billable_area(census.b, &record)?;
    let (words, _prep) = preflight(census.b, billable).await?;
    metrics.set(
        "preflight_b_billable",
        if billable == census.plot.area {
            "the area"
        } else if billable == 0 {
            "none"
        } else {
            "another"
        },
    );
    metrics.set("preflight_b", census.scene.scrub(&words));
    if !census.plot.owned {
        for key in OWNER_LEGS {
            for side in ["told_a", "told_b", "after"] {
                metrics.set(
                    &format!("{key}_{side}"),
                    "not measured: A owns no land here (gridspec-aditi-test-land)",
                );
            }
        }
        metrics.set(
            "deed_listing",
            "not measured: A owns no land here (gridspec-aditi-test-land)",
        );
        return Ok(());
    }

    // 2. A sale to B.
    let record = census.read(Who::A).await?;
    let sale = census.sale(&record, PRICE, census.scene.b);
    census.leg(metrics, "sale_to_b", Who::A, vec![sale]).await?;
    let short = census.buy(Who::B, PRICE.saturating_sub(1), b_group)?;
    census
        .leg(metrics, "buy_short", Who::B, vec![short])
        .await?;
    let buy = census.buy(Who::B, PRICE, b_group)?;
    census.leg(metrics, "buy", Who::B, vec![buy]).await?;

    // 3. Back to A.
    let record = census.read(Who::B).await?;
    if record.owner.uuid() == census.scene.b {
        let sale = census.sale(&record, PRICE, census.scene.a);
        census.leg(metrics, "sale_to_a", Who::B, vec![sale]).await?;
        let a_group = active_group(census.a).await?;
        let buy = census.buy(Who::A, PRICE, a_group)?;
        census.leg(metrics, "buy_back", Who::A, vec![buy]).await?;
    } else {
        return Err(TestFailure::Assertion(
            "B did not own the plot after buying it".to_owned(),
        ));
    }

    // 4. Passes.
    let record = census.read(Who::A).await?;
    let mut update = record.to_update();
    update.parcel_flags = update
        .parcel_flags
        .union(ParcelFlags::USE_ACCESS_LIST)
        .union(ParcelFlags::USE_PASS_LIST);
    update.pass_price = amount(PRICE);
    update.pass_hours = PASS_HOURS;
    census
        .leg(
            metrics,
            "pass_set",
            Who::A,
            vec![Command::UpdateParcel(Box::new(update))],
        )
        .await?;
    let plot = census.plot_for(Who::B)?;
    census
        .leg(
            metrics,
            "pass_buy",
            Who::B,
            vec![Command::BuyParcelPass { local_id: plot }],
        )
        .await?;
    census
        .leg(
            metrics,
            "pass_again",
            Who::B,
            vec![Command::BuyParcelPass { local_id: plot }],
        )
        .await?;
    let a_plot = census.plot_for(Who::A)?;
    census
        .leg(
            metrics,
            "pass_list",
            Who::A,
            vec![Command::RequestParcelAccessList {
                local_id: a_plot,
                scope: ParcelAccessScope::Access,
            }],
        )
        .await?;
    let record = census.read(Who::A).await?;
    let mut update = record.to_update();
    update.parcel_flags = update
        .parcel_flags
        .difference(ParcelFlags::USE_ACCESS_LIST)
        .difference(ParcelFlags::USE_PASS_LIST);
    update.pass_price = LindenAmount(0);
    census
        .leg(
            metrics,
            "pass_clear",
            Who::A,
            vec![
                Command::UpdateParcel(Box::new(update)),
                Command::UpdateParcelAccessList {
                    local_id: a_plot,
                    scope: ParcelAccessScope::Access,
                    entries: Vec::new(),
                },
            ],
        )
        .await?;

    // 5. A deed.
    let record = census.read(Who::A).await?;
    let mut update = record.to_update();
    update.group_id = Some(group);
    census
        .leg(
            metrics,
            "group_set",
            Who::A,
            vec![Command::UpdateParcel(Box::new(update))],
        )
        .await?;
    census
        .leg(
            metrics,
            "deed",
            Who::A,
            vec![Command::DeedParcelToGroup {
                local_id: a_plot,
                group_id: group,
            }],
        )
        .await?;
    let parcel_id = match census.plot.parcel_id {
        Some(parcel_id) => parcel_id,
        None => {
            let point = census.plot.point.clone();
            parcel_id_at(census.a, &point).await?
        }
    };
    tokio::time::sleep(LISTING_REST).await;
    let details = listing(census.a, parcel_id).await?;
    metrics.set(
        "deed_listing",
        format!(
            "flags {:#04x}, owner {}",
            details.flags.bits(),
            census.scene.whose(details.owner_id)
        ),
    );
    let record = census.read(Who::A).await?;
    let sale = census.sale(&record, 0, census.scene.a);
    census
        .leg(metrics, "group_sale_to_a", Who::A, vec![sale])
        .await?;
    let buy = census.buy(Who::A, 0, None)?;
    census
        .leg(metrics, "group_buy_back", Who::A, vec![buy])
        .await?;

    // 6. Abandon and reclaim.
    if is_opensim(census.grid) {
        let record = census.read(Who::A).await?;
        let sale = census.sale(&record, 0, census.scene.b);
        census
            .leg(metrics, "free_sale_to_b", Who::A, vec![sale])
            .await?;
        let buy = census.buy(Who::B, 0, b_group)?;
        census.leg(metrics, "free_buy", Who::B, vec![buy]).await?;
        let plot = census.plot_for(Who::B)?;
        census
            .leg(
                metrics,
                "release",
                Who::B,
                vec![Command::ReleaseParcel { local_id: plot }],
            )
            .await?;
        census
            .leg(
                metrics,
                "reclaim",
                Who::A,
                vec![Command::ReclaimParcel { local_id: a_plot }],
            )
            .await?;
    } else {
        for key in KEPT_PLOT_LEGS {
            for side in ["told_a", "told_b", "after"] {
                metrics.set(
                    &format!("{key}_{side}"),
                    "not run: Second Life's plot is kept",
                );
            }
        }
    }
    Ok(())
}

/// Leaves the plot as the census found it: A's, not for sale, no passes —
/// and on OpenSim, joined back into the region.
async fn clean_up(census: &mut Census<'_>) -> Result<(), TestFailure> {
    if !census.plot.owned {
        return Ok(());
    }
    let record = census.read(Who::A).await?;
    if record.owner.uuid() != census.scene.a {
        if is_opensim(census.grid) {
            let plot = census.plot_for(Who::A)?;
            census
                .a
                .send(Command::ReclaimParcel { local_id: plot })
                .await?;
        } else if record.owner.uuid() == census.scene.b {
            let record = census.read(Who::B).await?;
            let sale = census.sale(&record, 0, census.scene.a);
            census.b.send(sale).await?;
            tokio::time::sleep(Duration::from_secs(3)).await;
            let buy = census.buy(Who::A, 0, None)?;
            census.a.send(buy).await?;
        } else {
            let sale = census.sale(&record, 0, census.scene.a);
            census.a.send(sale).await?;
            tokio::time::sleep(Duration::from_secs(3)).await;
            let buy = census.buy(Who::A, 0, None)?;
            census.a.send(buy).await?;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let record = census.read(Who::A).await?;
    let mut update = record.to_update();
    update.parcel_flags = update
        .parcel_flags
        .difference(ParcelFlags::FOR_SALE)
        .difference(ParcelFlags::USE_ACCESS_LIST)
        .difference(ParcelFlags::USE_PASS_LIST);
    update.sale_price = Some(LindenAmount(0));
    update.auth_buyer_id = None;
    update.pass_price = LindenAmount(0);
    census
        .a
        .send(Command::UpdateParcel(Box::new(update)))
        .await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    if is_opensim(census.grid) {
        restore_single_parcel(census.a).await?;
    }
    let after = census.read(Who::A).await?;
    if after.owner.uuid() != census.scene.a {
        return Err(TestFailure::State(format!(
            "the clean-up left the plot {}",
            describe_parcel(&census.scene, &after)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Scene, raw_shape, without_figures};

    /// A time left is replaced; a whole number and a sentence's full stop are
    /// kept.
    #[test]
    fn a_figure_is_replaced() {
        assert_eq!(
            without_figures("a pass valid for 59.87 minutes. Pay L$1."),
            "a pass valid for <n> minutes. Pay L$1."
        );
    }

    /// A scrubbed text names neither avatar nor any id.
    #[test]
    fn a_scrubbed_text_names_nobody() {
        let scene = Scene {
            names: vec![("Some Body".to_owned(), "<A>"), ("Some".to_owned(), "<A>")],
            ..Scene::default()
        };
        assert_eq!(
            scene.scrub("Some Body paid c04f0117-0000-4000-8000-000000000001 L$1"),
            "<A> paid <id> L$1"
        );
    }

    /// The raw shape of a helper answer lists its keys, nested ones dotted,
    /// as the grid spelled them.
    #[test]
    fn a_helper_answer_is_described_by_its_keys() {
        let body = "<?xml version=\"1.0\"?><methodResponse><params><param><value><struct>\
                    <member><name>success</name><value><boolean>1</boolean></value></member>\
                    <member><name>landuse</name><value><struct><member><name>upgrade</name>\
                    <value><boolean>0</boolean></value></member></struct></value></member>\
                    </struct></value></param></params></methodResponse>";
        assert_eq!(raw_shape(body), "landuse.upgrade success");
    }
}
