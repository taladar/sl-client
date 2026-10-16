//! A census of directory search: what each grid answers to every search the
//! reference viewer's *Search* floater sends, over which road, how it pages and
//! filters by rating, what the avatar picker and the land holdings lookup
//! bring, and what the `ProductInfoRequest` capability names a land row's type.
//!
//! Avatar **A** searches; avatar **B** is the other avatar it looks for. One
//! question a leg:
//!
//! 1. **People** (`DirFindQuery` with `DFQ_PEOPLE`): A's own name, B's name
//!    with `DFQ_ONLINE`, an empty text, a two-letter text, and a common word
//!    on its first and second page.
//! 2. **Groups**: a common word, both pages.
//! 3. **Events**: the reference's upcoming-events query (`u|0|`).
//! 4. **Places** (`DirPlacesQuery`): the Linden-location browse, a common word
//!    on both pages, the same word for General only, and a two-letter text.
//! 5. **Land** (`DirLandQuery`): every parcel for sale on both pages, and
//!    General only; each row's land product (`ProductSKU`).
//! 6. **Classifieds** (`DirClassifiedQuery`): every rating in the classifieds'
//!    own bit space, and in the `DFQ_*` bits our client sent until this census.
//! 7. **The avatar picker** over UDP (A's name and a common word) and over the
//!    `AvatarPickerSearch` capability.
//! 8. **Land holdings** (`PlacesQuery` with `DFQ_AGENT_OWNED`).
//! 9. **Land products**: the `ProductInfoRequest` answer, raw and as the
//!    client reads it.
//!
//! Every leg is sent raw (`Command::Send`) where the client would take a
//! capability instead, and the circuit is probed throughout, so a reply's road
//! — UDP or the event queue — is recorded with it.
//!
//! Results of a public search change from day to day, so a metric never names
//! a row: it says how many came against the reference's page of 100, where the
//! nil rows stand, and how two answers relate.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/search.md`; every run is held to it.
//!
//! `2av`, live grids only: the fake grid answers no directory search yet
//! (`server-fake-grid-directory-search`).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AnyMessage, AvatarPickerResult, CircuitProbe, ClassifiedCategory, ClassifiedQueryFlags,
    Command, Diagnostic, DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult,
    DirLandResult, DirPeopleResult, DirPlaceResult, Event, LandProduct, LandSearchType,
    ParcelCategory, PlacesResult, QueryId, Reliability, TransactionId, Uuid,
};
use sl_wire::messages::{
    AvatarPickerRequest, AvatarPickerRequestAgentDataBlock, AvatarPickerRequestDataBlock,
    DirClassifiedQuery, DirClassifiedQueryAgentDataBlock, DirClassifiedQueryQueryDataBlock,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{hold_to_measured, http_client, shape_of};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, secs_metric};

/// How long a search is waited for at most.
const LISTEN: Duration = Duration::from_secs(12);

/// How long the census listens on after a search's first answer, for the rest
/// of a page that comes as several messages.
const QUIET: Duration = Duration::from_secs(3);

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// The reference viewer's page of results (`RESULT_PAGE_SIZE`).
const PAGE: usize = 100;

/// A word common enough in names that a grid with a search index has more
/// than a page of everything for it.
const COMMON_WORD: &str = "linden";

/// A text too short for the reference to send (`MIN_SEARCH_STRING_SIZE` is
/// two, and it refuses a text of that length or less).
const SHORT_TEXT: &str = "ab";

/// The capability that searches residents by name.
const AVATAR_PICKER_SEARCH: &str = "AvatarPickerSearch";

/// The capability that names the land products.
const PRODUCT_INFO_REQUEST: &str = "ProductInfoRequest";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/search.md (search-directory, 2026-10-16)";

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
/// Not held: how long the census took, and the land products' full list
/// (`product_info_list`, recorded for the fake grid to serve). Held as
/// [`Answer::Any`]: how a places search's second page relates to its first on
/// Second Life, which turns on the parcels' traffic at the moment of asking.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "classifieds_dfq_bits",
        each(
            Answer::Text("1 DirClassifiedReply(s) over UDP: no rows, no nil row, status 0x4"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "classifieds_dfq_bits_against_every_rating",
        both(Answer::Text("neither has a row")),
    ),
    (
        "classifieds_every_rating",
        each(
            Answer::Text("1 DirClassifiedReply(s) over UDP: no rows, no nil row, status 0x4"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "events_upcoming",
        each(
            Answer::Text("1 DirEventsReply(s) over UDP: 1 row, the one row nil, status 0x4"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "groups_common_page_1",
        each(
            Answer::Text("several DirGroupsReply(s) over UDP: a page and one more row, no nil row"),
            Answer::Text("1 DirGroupsReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "groups_common_page_2",
        each(
            Answer::Text("several DirGroupsReply(s) over UDP: a page and one more row, no nil row"),
            Answer::Text("1 DirGroupsReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "groups_common_page_2_against_page_1",
        each(
            Answer::Text("one of the same rows and others"),
            Answer::Text("neither has a row"),
        ),
    ),
    (
        "holdings_own",
        each(
            Answer::Text("1 PlacesReply(s) over the event queue: 1 row, the one row nil"),
            Answer::Text("nothing"),
        ),
    ),
    ("holdings_own_products", both(Answer::Text("no real row"))),
    (
        "land_for_sale_page_1",
        each(
            Answer::Text(
                "1 DirLandReply(s) over the event queue: fewer than a page of rows, no nil row",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "land_for_sale_page_2",
        each(
            Answer::Text("1 DirLandReply(s) over the event queue: 1 row, the one row nil"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "land_for_sale_page_2_against_page_1",
        each(
            Answer::Text("none of its own"),
            Answer::Text("neither has a row"),
        ),
    ),
    (
        "land_for_sale_products",
        each(
            Answer::Text("every row has one, each a product the list names"),
            Answer::Text("no real row"),
        ),
    ),
    (
        "land_general_only",
        each(
            Answer::Text(
                "1 DirLandReply(s) over the event queue: fewer than a page of rows, no nil row",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "land_general_only_against_every_rating",
        each(
            Answer::Text("some of the same rows"),
            Answer::Text("neither has a row"),
        ),
    ),
    ("login_search_url", both(Answer::Text("none"))),
    (
        "people_common_page_1",
        each(
            Answer::Text("several DirPeopleReply(s) over UDP: a page and one more row, no nil row"),
            Answer::Text("1 DirPeopleReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "people_common_page_2",
        each(
            Answer::Text("several DirPeopleReply(s) over UDP: a page and one more row, no nil row"),
            Answer::Text("1 DirPeopleReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "people_common_page_2_against_page_1",
        each(
            Answer::Text("one of the same rows and others"),
            Answer::Text("neither has a row"),
        ),
    ),
    (
        "people_empty_text",
        each(
            Answer::Text("nothing"),
            Answer::Text("several DirPeopleReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "people_legacy_fields",
        both(Answer::Text("group empty and reputation 0 on every row")),
    ),
    (
        "people_other_online",
        each(
            Answer::Text("1 DirPeopleReply(s) over UDP: 1 row, no nil row"),
            Answer::Text("1 DirPeopleReply(s) over UDP: fewer than a page of rows, no nil row"),
        ),
    ),
    (
        "people_other_online_other",
        both(Answer::Text("present, offline")),
    ),
    (
        "people_own_name",
        each(
            Answer::Text("1 DirPeopleReply(s) over UDP: 1 row, no nil row"),
            Answer::Text("1 DirPeopleReply(s) over UDP: fewer than a page of rows, no nil row"),
        ),
    ),
    (
        "people_own_name_self",
        both(Answer::Text("present, offline")),
    ),
    (
        "people_short_text",
        each(
            Answer::Text("nothing"),
            Answer::Text("1 DirPeopleReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "picker_cap_own_name",
        each(
            Answer::Text(
                "200, 1 agent(s), an agent {display_name:string,display_name_next_update:date,id:uuid,is_display_name_default:boolean,legacy_first_name:string,legacy_last_name:string,username:string}",
            ),
            Answer::Text(
                "200, 3 agent(s), an agent {display_name:string,id:uuid,is_display_name_default:boolean,legacy_first_name:string,legacy_last_name:string,username:string}",
            ),
        ),
    ),
    ("picker_cap_own_name_self", both(Answer::Text("present"))),
    ("picker_client_own_name_self", both(Answer::Text("present"))),
    (
        "picker_udp_common",
        each(
            Answer::Text("1 AvatarPickerReply(s) over UDP: 1 row, the one row nil"),
            Answer::Text("1 AvatarPickerReply(s) over UDP: no rows, no nil row"),
        ),
    ),
    (
        "picker_udp_own_name",
        each(
            Answer::Text("1 AvatarPickerReply(s) over UDP: 1 row, the one row nil"),
            Answer::Text("1 AvatarPickerReply(s) over UDP: fewer than a page of rows, no nil row"),
        ),
    ),
    (
        "picker_udp_own_name_self",
        each(Answer::Text("absent"), Answer::Text("present")),
    ),
    (
        "places_category_none",
        each(
            Answer::Text(
                "several DirPlacesReply(s) over UDP: a page and one more row, no nil row, status 0x0",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "places_category_none_against_any",
        each(
            Answer::Text("several of the same rows and others"),
            Answer::Text("neither has a row"),
        ),
    ),
    (
        "places_common_page_1",
        each(
            Answer::Text(
                "several DirPlacesReply(s) over UDP: a page and one more row, no nil row, status 0x0",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "places_common_page_2",
        each(
            Answer::Text(
                "several DirPlacesReply(s) over UDP: a page and one more row, no nil row, status 0x0",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "places_common_page_2_against_page_1",
        each(Answer::Any, Answer::Text("neither has a row")),
    ),
    (
        "places_general_only",
        each(
            Answer::Text(
                "several DirPlacesReply(s) over UDP: a page and one more row, no nil row, status 0x0",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "places_general_only_against_every_rating",
        each(
            Answer::Text("several of the same rows and others"),
            Answer::Text("neither has a row"),
        ),
    ),
    (
        "places_linden_locations",
        each(
            Answer::Text(
                "several DirPlacesReply(s) over UDP: a page and one more row, no nil row, status 0x0",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "places_short_text",
        each(
            Answer::Text("1 DirPlacesReply(s) over UDP: 1 row, the one row nil, status 0x2"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "product_info_answer",
        each(
            Answer::Text(
                "200, an array, 26 entr(ies), an entry {description:string,name:string,sku:string}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "product_info_client",
        each(
            Answer::Text("26 product(s), 26 with a description"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "product_info_skus",
        each(
            Answer::Text(
                "023,024,027,029,121,123,125,127,129,131,132,133,141,221,224,229,241,324,329,424,524,624,733,833,841,844",
            ),
            Answer::Text("no capability"),
        ),
    ),
];

/// Which kind of directory reply a leg waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `DirPeopleReply`.
    People,
    /// `DirGroupsReply`.
    Groups,
    /// `DirEventsReply`.
    Events,
    /// `DirPlacesReply`.
    Places,
    /// `DirLandReply`.
    Land,
    /// `DirClassifiedReply`.
    Classifieds,
    /// `AvatarPickerReply`.
    Picker,
    /// `PlacesReply`.
    Holdings,
}

impl Kind {
    /// The reply message's name, as a datagram and an event-queue message are
    /// named.
    const fn message(self) -> &'static str {
        match self {
            Self::People => "DirPeopleReply",
            Self::Groups => "DirGroupsReply",
            Self::Events => "DirEventsReply",
            Self::Places => "DirPlacesReply",
            Self::Land => "DirLandReply",
            Self::Classifieds => "DirClassifiedReply",
            Self::Picker => "AvatarPickerReply",
            Self::Holdings => "PlacesReply",
        }
    }
}

/// One row of a directory reply, as far as a metric looks at it.
#[derive(Debug, Clone)]
struct Row {
    /// The row's subject: the avatar, group, parcel, event owner or
    /// classified.
    id: Uuid,
    /// Whether the row says its avatar is online (people rows).
    online: Option<bool>,
    /// The row's land product (land and holdings rows from the event queue).
    sku: Option<String>,
    /// Whether the row's legacy fields are what a modern grid sends: an empty
    /// group text and a zero reputation (people rows).
    legacy_fields_blank: Option<bool>,
}

/// One directory reply the census was sent.
#[derive(Debug, Clone)]
struct Reply {
    /// The query it answers.
    query_id: Uuid,
    /// Its rows, in order.
    rows: Vec<Row>,
    /// Its search status, where the message has one.
    status: Option<u32>,
}

/// What the session was sent during a leg.
#[derive(Debug, Default)]
struct Got {
    /// Every directory reply.
    replies: Vec<Reply>,
    /// Every alert.
    alerts: Vec<String>,
    /// Every land product list.
    products: Vec<Vec<LandProduct>>,
}

impl Got {
    /// Whether any reply answers `query_id`.
    fn answered(&self, query_id: Uuid) -> bool {
        self.replies.iter().any(|reply| reply.query_id == query_id)
    }
}

/// A people row.
fn person(row: &DirPeopleResult) -> Row {
    Row {
        id: row.agent_id.uuid(),
        online: Some(row.online),
        sku: None,
        legacy_fields_blank: Some(
            row.group.trim_end_matches('\0').is_empty() && row.reputation == 0,
        ),
    }
}

/// A row with nothing but its subject.
const fn plain(id: Uuid) -> Row {
    Row {
        id,
        online: None,
        sku: None,
        legacy_fields_blank: None,
    }
}

/// A group row.
const fn group(row: &DirGroupResult) -> Row {
    plain(row.group_id.uuid())
}

/// An event row.
const fn event(row: &DirEventResult) -> Row {
    plain(row.owner_id)
}

/// A places row.
const fn place(row: &DirPlaceResult) -> Row {
    plain(row.parcel_id.uuid())
}

/// A land row.
fn land_row(row: &DirLandResult) -> Row {
    Row {
        sku: row.product_sku.clone(),
        ..plain(row.parcel_id.uuid())
    }
}

/// A classified row.
const fn classified(row: &DirClassifiedResult) -> Row {
    plain(row.classified_id.uuid())
}

/// An avatar picker row.
const fn picked(row: &AvatarPickerResult) -> Row {
    plain(row.avatar_id.uuid())
}

/// A land holdings row.
fn holding(row: &PlacesResult) -> Row {
    Row {
        sku: row.product_sku.clone(),
        ..plain(row.owner_id)
    }
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event_seen: &Event) -> Option<()> {
    let reply = |_kind: Kind, query_id: &Uuid, rows: Vec<Row>, status: Option<u32>| Reply {
        query_id: *query_id,
        rows,
        status,
    };
    let answer = match event_seen {
        Event::DirPeopleReply { query_id, results } => reply(
            Kind::People,
            query_id,
            results.iter().map(person).collect(),
            None,
        ),
        Event::DirGroupsReply { query_id, results } => reply(
            Kind::Groups,
            query_id,
            results.iter().map(group).collect(),
            None,
        ),
        Event::DirEventsReply {
            query_id,
            results,
            status,
        } => reply(
            Kind::Events,
            query_id,
            results.iter().map(event).collect(),
            Some(*status),
        ),
        Event::DirPlacesReply {
            query_id,
            results,
            status,
        } => reply(
            Kind::Places,
            query_id,
            results.iter().map(place).collect(),
            Some(*status),
        ),
        Event::DirLandReply { query_id, results } => reply(
            Kind::Land,
            query_id,
            results.iter().map(land_row).collect(),
            None,
        ),
        Event::DirClassifiedReply {
            query_id,
            results,
            status,
        } => reply(
            Kind::Classifieds,
            query_id,
            results.iter().map(classified).collect(),
            Some(*status),
        ),
        Event::AvatarPickerReply { query_id, results } => reply(
            Kind::Picker,
            query_id,
            results.iter().map(picked).collect(),
            None,
        ),
        Event::PlacesReply {
            query_id, results, ..
        } => reply(
            Kind::Holdings,
            query_id,
            results.iter().map(holding).collect(),
            None,
        ),
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            got.alerts.push(message.clone());
            return Some(());
        }
        Event::LandProducts(products) => {
            got.products.push(products.clone());
            return Some(());
        }
        _ => return None,
    };
    got.replies.push(answer);
    Some(())
}

/// What one search brought: the replies to it, and the roads they came by.
#[derive(Debug, Default)]
struct Leg {
    /// Everything the session was sent meanwhile.
    got: Got,
    /// How many datagrams of the reply's message arrived meanwhile.
    datagrams: usize,
    /// The event-queue messages the client could not read meanwhile.
    unread: Vec<String>,
}

impl Leg {
    /// The replies to `query_id`, in order.
    fn replies(&self, query_id: Uuid) -> Vec<&Reply> {
        self.got
            .replies
            .iter()
            .filter(|reply| reply.query_id == query_id)
            .collect()
    }

    /// Every row of the replies to `query_id`, in order.
    fn rows(&self, query_id: Uuid) -> Vec<&Row> {
        self.replies(query_id)
            .into_iter()
            .flat_map(|reply| reply.rows.iter())
            .collect()
    }
}

/// Sends `command` on `session` and listens for the answers to `query_id` of
/// `kind`, with the roads they came by.
async fn ask(
    session: &mut Session,
    command: Command,
    query_id: Uuid,
    kind: Kind,
) -> Result<Leg, TestFailure> {
    ask_until(session, command, kind, |got| got.answered(query_id)).await
}

/// Sends `command` on `session` and listens until `done` says the answer is
/// in, with the roads the answers of `kind` came by.
async fn ask_until(
    session: &mut Session,
    command: Command,
    kind: Kind,
    done: impl Fn(&Got) -> bool,
) -> Result<Leg, TestFailure> {
    let before = session.diagnostics().len();
    session.send(command).await?;
    let got = watch_for(session, done).await?;
    // The circuit probe's datagram reports trail the events a little.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let mut datagrams = 0_usize;
    let mut unread = Vec::new();
    for diagnostic in session.diagnostics().iter().skip(before) {
        match diagnostic {
            Diagnostic::Datagram { name, .. } if *name == Some(kind.message()) => {
                datagrams = datagrams.saturating_add(1);
            }
            Diagnostic::UnknownCapsEvent { message } => {
                unread.push(format!("unread event-queue message {message}"));
            }
            Diagnostic::CapsDecodeFailed { message, .. } => {
                unread.push(format!("event-queue message {message} that did not decode"));
            }
            _ => {}
        }
    }
    unread.sort();
    unread.dedup();
    Ok(Leg {
        got,
        datagrams,
        unread,
    })
}

/// Listens on `session` until `done` says the answer is in and [`QUIET`] more
/// has passed, or for [`LISTEN`] at most.
async fn watch_for(session: &mut Session, done: impl Fn(&Got) -> bool) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        if done_at.is_none() && done(&got) {
            done_at = Some(Instant::now());
        }
        let remaining = done_at.map_or_else(
            || LISTEN.saturating_sub(started.elapsed()),
            |at| QUIET.saturating_sub(at.elapsed()),
        );
        if remaining.is_zero() {
            return Ok(got);
        }
        match session
            .wait_for(remaining, |event_seen| fold(&mut got, event_seen))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(other) => return Err(other),
        }
    }
}

/// How many rows came, against the reference's page.
fn count_text(rows: usize) -> String {
    match rows {
        0 => "no rows".to_owned(),
        1 => "1 row".to_owned(),
        n if n < PAGE => "fewer than a page of rows".to_owned(),
        PAGE => "a page of rows".to_owned(),
        n if n == PAGE.saturating_add(1) => "a page and one more row".to_owned(),
        _ => "more than a page and one rows".to_owned(),
    }
}

/// Where the nil rows of `rows` stand.
fn nil_text(rows: &[&Row]) -> String {
    let nil: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_index, row)| row.id.is_nil())
        .map(|(index, _row)| index)
        .collect();
    match nil.as_slice() {
        [] => "no nil row".to_owned(),
        [only] if rows.len() == 1 && *only == 0 => "the one row nil".to_owned(),
        [only] if only.saturating_add(1) == rows.len() => "the last row nil".to_owned(),
        [_only] => "one nil row amid".to_owned(),
        many if many.len() == rows.len() => "every row nil".to_owned(),
        many => format!("{} nil rows", many.len()),
    }
}

/// The road and the shape of the answer to `query_id` of `kind`.
fn answer_text(leg: &Leg, query_id: Uuid, kind: Kind) -> String {
    let replies = leg.replies(query_id);
    let unread = if leg.unread.is_empty() {
        String::new()
    } else {
        format!("; {}", leg.unread.join(", "))
    };
    if replies.is_empty() {
        return format!("nothing{unread}");
    }
    let road = match leg.datagrams {
        0 => "the event queue",
        n if n >= replies.len() => "UDP",
        _ => "UDP and the event queue",
    };
    let rows = leg.rows(query_id);
    let statuses: BTreeSet<u32> = replies.iter().filter_map(|reply| reply.status).collect();
    let status = if statuses.is_empty() {
        String::new()
    } else {
        let statuses: Vec<String> = statuses
            .iter()
            .map(|status| format!("{status:#x}"))
            .collect();
        format!(", status {}", statuses.join("/"))
    };
    // How many messages a page takes turns on how long its rows' names are.
    let messages = if replies.len() == 1 { "1" } else { "several" };
    format!(
        "{messages} {}(s) over {road}: {}, {}{status}{unread}",
        kind.message(),
        count_text(rows.len()),
        nil_text(&rows),
    )
}

/// How the real rows of `second` relate to those of `first`.
fn relation_text(first: &[&Row], second: &[&Row]) -> &'static str {
    let ids = |rows: &[&Row]| -> BTreeSet<Uuid> {
        rows.iter()
            .map(|row| row.id)
            .filter(|id| !id.is_nil())
            .collect()
    };
    let (first, second) = (ids(first), ids(second));
    if first.is_empty() && second.is_empty() {
        "neither has a row"
    } else if second.is_empty() {
        "none of its own"
    } else if first == second {
        "the same rows"
    } else if second.is_subset(&first) {
        "some of the same rows"
    } else if second.is_superset(&first) {
        "every one of the rows and others"
    } else {
        match second.intersection(&first).count() {
            0 => "other rows only",
            1 => "one of the same rows and others",
            _ => "several of the same rows and others",
        }
    }
}

/// The name a resident is searched for by: the legacy name, or a Resident's
/// first name alone, as a Resident is known.
fn search_name(legacy: &str) -> String {
    match legacy.split_once(' ') {
        Some((first, last)) if last.eq_ignore_ascii_case("Resident") => first.to_owned(),
        _ => legacy.to_owned(),
    }
}

/// A new query id.
fn new_query() -> Uuid {
    Uuid::new_v4()
}

/// Every rating, in the `DFQ_*` bits.
const EVERY_RATING: DirFindFlags = DirFindFlags::INC_PG
    .union(DirFindFlags::INC_MATURE)
    .union(DirFindFlags::INC_ADULT);

/// A capability's URL on `session`'s region, waited for as long as the
/// capability map may take; `None` where the region does not grant it.
async fn cap(session: &Session, name: &str) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Some(url) = session.cap(name) {
            return Some(url);
        }
        if session.capability_names().is_empty() && started.elapsed() < CAP_WAIT {
            tokio::time::sleep(Duration::from_millis(250)).await;
        } else {
            return None;
        }
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

/// `text` NUL-terminated, as the reference packs a string field.
fn nul_terminated(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// A raw `command` for `message`, sent reliably.
fn raw(message: AnyMessage) -> Command {
    Command::Send {
        message: Box::new(message),
        reliability: Reliability::Reliable,
    }
}

/// A census of directory search: every search the reference sends, the
/// avatar picker, the land holdings and the land products.
#[derive(Debug)]
pub struct SearchDirectory;

impl GridTest for SearchDirectory {
    fn name(&self) -> &'static str {
        "search-directory"
    }

    fn description(&self) -> &'static str {
        "What each directory search, the avatar picker, the land holdings and ProductInfoRequest answer, over which road"
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

/// The census: both avatars in, the probe on, the legs.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "search-directory needs a secondary avatar".to_owned(),
        ));
    };
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_id = ids_of(a)?.0;
    let b_id = ids_of(b)?.0;
    let b_name = search_name(&b.avatar_legacy_name());
    metrics.set(
        "login_search_url",
        a.login_success()
            .and_then(|login| login.search_url.as_ref())
            .map_or("none", |_url| "a URL"),
    );
    a.send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    let started = Instant::now();
    let outcome = legs(a, a_id, b_id, &b_name, metrics).await;
    metrics.set_timing(&secs_metric("census"), started.elapsed().as_secs_f64());
    a.send(Command::ProbeCircuits(CircuitProbe::Off)).await?;
    outcome
}

/// Every leg, in order.
async fn legs(
    a: &mut Session,
    a_id: Uuid,
    b_id: Uuid,
    b_name: &str,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    people(a, a_id, b_id, b_name, metrics).await?;
    groups(a, metrics).await?;
    events(a, metrics).await?;
    places(a, metrics).await?;
    let products = product_info(a, metrics).await?;
    land(a, &products, metrics).await?;
    classifieds(a, metrics).await?;
    picker(a, a_id, metrics).await?;
    holdings(a, &products, metrics).await?;
    Ok(())
}

/// A `DirFindQuery`.
fn dir_find(query_id: Uuid, text: &str, flags: DirFindFlags, start: i32) -> Command {
    Command::DirFindQuery {
        query_id: QueryId::from(query_id),
        query_text: text.to_owned(),
        flags,
        query_start: start,
    }
}

/// Leg 1: people.
async fn people(
    a: &mut Session,
    a_id: Uuid,
    b_id: Uuid,
    b_name: &str,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let own_name = search_name(&a.avatar_legacy_name());
    let id = new_query();
    let leg = ask(
        a,
        dir_find(id, &own_name, DirFindFlags::PEOPLE, 0),
        id,
        Kind::People,
    )
    .await?;
    metrics.set("people_own_name", answer_text(&leg, id, Kind::People));
    let rows = leg.rows(id);
    metrics.set("people_own_name_self", subject_text(&rows, a_id));
    metrics.set(
        "people_legacy_fields",
        match rows
            .iter()
            .filter(|row| !row.id.is_nil())
            .filter_map(|row| row.legacy_fields_blank)
            .collect::<BTreeSet<bool>>()
            .into_iter()
            .collect::<Vec<bool>>()
            .as_slice()
        {
            [] => "no real row",
            [true] => "group empty and reputation 0 on every row",
            [false] => "group or reputation set on every row",
            _ => "group or reputation set on some rows",
        },
    );

    let id = new_query();
    let leg = ask(
        a,
        dir_find(
            id,
            b_name,
            DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE),
            0,
        ),
        id,
        Kind::People,
    )
    .await?;
    metrics.set("people_other_online", answer_text(&leg, id, Kind::People));
    metrics.set(
        "people_other_online_other",
        subject_text(&leg.rows(id), b_id),
    );

    let id = new_query();
    let leg = ask(
        a,
        dir_find(id, "", DirFindFlags::PEOPLE, 0),
        id,
        Kind::People,
    )
    .await?;
    metrics.set("people_empty_text", answer_text(&leg, id, Kind::People));

    let id = new_query();
    let leg = ask(
        a,
        dir_find(id, SHORT_TEXT, DirFindFlags::PEOPLE, 0),
        id,
        Kind::People,
    )
    .await?;
    metrics.set("people_short_text", answer_text(&leg, id, Kind::People));

    let (first, second) = two_pages(a, Kind::People, |id, start| {
        dir_find(id, COMMON_WORD, DirFindFlags::PEOPLE, start)
    })
    .await?;
    record_pages(metrics, "people_common", Kind::People, &first, &second);
    Ok(())
}

/// What `rows` say of `who`: whether among them, and whether online.
fn subject_text(rows: &[&Row], who: Uuid) -> String {
    match rows.iter().find(|row| row.id == who) {
        None => "absent".to_owned(),
        Some(row) => match row.online {
            Some(true) => "present, online".to_owned(),
            Some(false) => "present, offline".to_owned(),
            None => "present".to_owned(),
        },
    }
}

/// One search's answer: the query it was sent under and what came.
struct Asked {
    /// The query id.
    id: Uuid,
    /// What came.
    leg: Leg,
}

/// Sends the search `build` makes for the first page (start 0) and for the
/// second (start [`PAGE`]).
async fn two_pages(
    a: &mut Session,
    kind: Kind,
    build: impl Fn(Uuid, i32) -> Command,
) -> Result<(Asked, Asked), TestFailure> {
    let page = i32::try_from(PAGE).unwrap_or(i32::MAX);
    let id = new_query();
    let leg = ask(a, build(id, 0), id, kind).await?;
    let first = Asked { id, leg };
    let id = new_query();
    let leg = ask(a, build(id, page), id, kind).await?;
    Ok((first, Asked { id, leg }))
}

/// Records a search's two pages under `key`: each page's answer, and how the
/// second page's rows relate to the first's.
fn record_pages(metrics: &mut Metrics, key: &str, kind: Kind, first: &Asked, second: &Asked) {
    metrics.set(
        &format!("{key}_page_1"),
        answer_text(&first.leg, first.id, kind),
    );
    metrics.set(
        &format!("{key}_page_2"),
        answer_text(&second.leg, second.id, kind),
    );
    metrics.set(
        &format!("{key}_page_2_against_page_1"),
        relation_text(&first.leg.rows(first.id), &second.leg.rows(second.id)),
    );
}

/// Leg 2: groups.
async fn groups(a: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let (first, second) = two_pages(a, Kind::Groups, |id, start| {
        dir_find(
            id,
            COMMON_WORD,
            DirFindFlags::GROUPS.union(EVERY_RATING),
            start,
        )
    })
    .await?;
    record_pages(metrics, "groups_common", Kind::Groups, &first, &second);
    Ok(())
}

/// Leg 3: events, as the reference asks for the upcoming ones of every
/// category.
async fn events(a: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let id = new_query();
    let leg = ask(
        a,
        dir_find(id, "u|0|", DirFindFlags::DATE_EVENTS.union(EVERY_RATING), 0),
        id,
        Kind::Events,
    )
    .await?;
    metrics.set("events_upcoming", answer_text(&leg, id, Kind::Events));
    Ok(())
}

/// A `DirPlacesQuery`.
fn dir_places(
    query_id: Uuid,
    text: &str,
    flags: DirFindFlags,
    category: ParcelCategory,
    start: i32,
) -> Command {
    Command::DirPlacesQuery {
        query_id: QueryId::from(query_id),
        query_text: text.to_owned(),
        flags,
        category,
        sim_name: String::new(),
        query_start: start,
    }
}

/// Leg 4: places.
async fn places(a: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let id = new_query();
    let leg = ask(
        a,
        dir_places(
            id,
            "",
            DirFindFlags::INC_PG
                .union(DirFindFlags::INC_MATURE)
                .union(DirFindFlags::DWELL_SORT),
            ParcelCategory::Linden,
            0,
        ),
        id,
        Kind::Places,
    )
    .await?;
    metrics.set(
        "places_linden_locations",
        answer_text(&leg, id, Kind::Places),
    );

    let every = EVERY_RATING.union(DirFindFlags::DWELL_SORT);
    let (first, second) = two_pages(a, Kind::Places, |id, start| {
        dir_places(id, COMMON_WORD, every, ParcelCategory::Any, start)
    })
    .await?;
    record_pages(metrics, "places_common", Kind::Places, &first, &second);

    // Category `C_NONE`, which our viewer sent for "Any" until this census.
    let id = new_query();
    let none = ask(
        a,
        dir_places(id, COMMON_WORD, every, ParcelCategory::None, 0),
        id,
        Kind::Places,
    )
    .await?;
    metrics.set("places_category_none", answer_text(&none, id, Kind::Places));
    metrics.set(
        "places_category_none_against_any",
        relation_text(&first.leg.rows(first.id), &none.rows(id)),
    );

    let id = new_query();
    let general = ask(
        a,
        dir_places(
            id,
            COMMON_WORD,
            DirFindFlags::INC_PG.union(DirFindFlags::DWELL_SORT),
            ParcelCategory::Any,
            0,
        ),
        id,
        Kind::Places,
    )
    .await?;
    metrics.set(
        "places_general_only",
        answer_text(&general, id, Kind::Places),
    );
    metrics.set(
        "places_general_only_against_every_rating",
        relation_text(&first.leg.rows(first.id), &general.rows(id)),
    );

    let id = new_query();
    let leg = ask(
        a,
        dir_places(id, SHORT_TEXT, every, ParcelCategory::Any, 0),
        id,
        Kind::Places,
    )
    .await?;
    metrics.set("places_short_text", answer_text(&leg, id, Kind::Places));
    Ok(())
}

/// A `DirLandQuery` for every parcel for sale, dearest first.
fn dir_land(query_id: Uuid, ratings: DirFindFlags, start: i32) -> Command {
    Command::DirLandQuery {
        query_id: QueryId::from(query_id),
        flags: ratings
            .union(DirFindFlags::FOR_SALE)
            .union(DirFindFlags::PRICE_SORT),
        search_type: LandSearchType::ALL,
        price: 0,
        area: 0,
        query_start: start,
    }
}

/// What the land products of `rows` are: whether each has one, and whether
/// `products` names it.
fn sku_text(rows: &[&Row], products: &[LandProduct]) -> &'static str {
    let real: Vec<&&Row> = rows.iter().filter(|row| !row.id.is_nil()).collect();
    if real.is_empty() {
        return "no real row";
    }
    let with: Vec<&str> = real.iter().filter_map(|row| row.sku.as_deref()).collect();
    if with.is_empty() {
        return "no row has one";
    }
    let named = with
        .iter()
        .all(|sku| products.iter().any(|product| product.sku == *sku));
    match (with.len() == real.len(), named) {
        (true, true) => "every row has one, each a product the list names",
        (true, false) => "every row has one, some the list does not name",
        (false, true) => "some rows have one, each a product the list names",
        (false, false) => "some rows have one, some the list does not name",
    }
}

/// Leg 5: land for sale.
async fn land(
    a: &mut Session,
    products: &[LandProduct],
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let (first, second) =
        two_pages(a, Kind::Land, |id, start| dir_land(id, EVERY_RATING, start)).await?;
    record_pages(metrics, "land_for_sale", Kind::Land, &first, &second);
    metrics.set(
        "land_for_sale_products",
        sku_text(&first.leg.rows(first.id), products),
    );
    let id = new_query();
    let general = ask(a, dir_land(id, DirFindFlags::INC_PG, 0), id, Kind::Land).await?;
    metrics.set("land_general_only", answer_text(&general, id, Kind::Land));
    metrics.set(
        "land_general_only_against_every_rating",
        relation_text(&first.leg.rows(first.id), &general.rows(id)),
    );
    Ok(())
}

/// A raw `DirClassifiedQuery` with `flags`, whatever bit space they are in.
fn dir_classified(a: &Session, query_id: Uuid, flags: u32) -> Result<Command, TestFailure> {
    let (agent_id, session_id) = ids_of(a)?;
    Ok(raw(AnyMessage::DirClassifiedQuery(DirClassifiedQuery {
        agent_data: DirClassifiedQueryAgentDataBlock {
            agent_id,
            session_id,
        },
        query_data: DirClassifiedQueryQueryDataBlock {
            query_id,
            query_text: nul_terminated(""),
            query_flags: flags,
            category: ClassifiedCategory::AnyCategory.to_u32(),
            query_start: 0,
        },
    })))
}

/// Leg 6: classifieds, in the classifieds' bit space and in the `DFQ_*` bits.
async fn classifieds(a: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let id = new_query();
    let command = dir_classified(
        a,
        id,
        ClassifiedQueryFlags::for_ratings(true, true, true).bits(),
    )?;
    let reference = ask(a, command, id, Kind::Classifieds).await?;
    metrics.set(
        "classifieds_every_rating",
        answer_text(&reference, id, Kind::Classifieds),
    );
    let reference_id = id;

    let id = new_query();
    let command = dir_classified(a, id, EVERY_RATING.bits())?;
    let dfq = ask(a, command, id, Kind::Classifieds).await?;
    metrics.set(
        "classifieds_dfq_bits",
        answer_text(&dfq, id, Kind::Classifieds),
    );
    metrics.set(
        "classifieds_dfq_bits_against_every_rating",
        relation_text(&reference.rows(reference_id), &dfq.rows(id)),
    );
    Ok(())
}

/// A raw `AvatarPickerRequest` for `name`.
fn picker_request(a: &Session, query_id: Uuid, name: &str) -> Result<Command, TestFailure> {
    let (agent_id, session_id) = ids_of(a)?;
    Ok(raw(AnyMessage::AvatarPickerRequest(AvatarPickerRequest {
        agent_data: AvatarPickerRequestAgentDataBlock {
            agent_id,
            session_id,
            query_id,
        },
        data: AvatarPickerRequestDataBlock {
            name: nul_terminated(name),
        },
    })))
}

/// Leg 7: the avatar picker, over UDP and over the capability.
async fn picker(a: &mut Session, a_id: Uuid, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let own_name = search_name(&a.avatar_legacy_name());
    let id = new_query();
    let command = picker_request(a, id, &own_name)?;
    let leg = ask(a, command, id, Kind::Picker).await?;
    metrics.set("picker_udp_own_name", answer_text(&leg, id, Kind::Picker));
    metrics.set(
        "picker_udp_own_name_self",
        subject_text(&leg.rows(id), a_id),
    );

    let id = new_query();
    let command = picker_request(a, id, COMMON_WORD)?;
    let leg = ask(a, command, id, Kind::Picker).await?;
    metrics.set("picker_udp_common", answer_text(&leg, id, Kind::Picker));

    match cap(a, AVATAR_PICKER_SEARCH).await {
        None => {
            metrics.set("picker_cap_own_name", NO_CAPABILITY);
            metrics.set("picker_cap_own_name_self", NO_CAPABILITY);
        }
        Some(url) => {
            let base = if url.ends_with('/') {
                url
            } else {
                format!("{url}/")
            };
            let escaped: String =
                url::form_urlencoded::byte_serialize(own_name.replace('.', " ").as_bytes())
                    .collect();
            let response = http_client()?
                .get(format!("{base}?page_size=100&names={escaped}"))
                .header("Accept", "application/llsd+xml")
                .send()
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("a capability request failed: {error}"))
                })?;
            let status = response.status().as_u16();
            let llsd = parse_llsd_xml(&response.text().await.unwrap_or_default()).ok();
            let agents: Vec<Llsd> = match llsd.as_ref().and_then(|llsd| llsd.get("agents")) {
                Some(Llsd::Array(agents)) => agents.clone(),
                _ => Vec::new(),
            };
            metrics.set(
                "picker_cap_own_name",
                format!(
                    "{status}, {} agent(s), an agent {}",
                    agents.len(),
                    agents.first().map_or_else(|| "absent".to_owned(), shape_of)
                ),
            );
            metrics.set(
                "picker_cap_own_name_self",
                if agents
                    .iter()
                    .any(|agent| matches!(agent.get("id"), Some(Llsd::Uuid(id)) if *id == a_id))
                {
                    "present"
                } else {
                    "absent"
                },
            );
        }
    }

    // The client's own command, which takes the capability where it is
    // granted.
    let id = new_query();
    let leg = ask(
        a,
        Command::AvatarPickerRequest {
            query_id: QueryId::from(id),
            name: own_name,
        },
        id,
        Kind::Picker,
    )
    .await?;
    metrics.set(
        "picker_client_own_name_self",
        subject_text(&leg.rows(id), a_id),
    );
    Ok(())
}

/// Leg 8: the land holdings, as the reference's land holdings floater asks
/// for them: the nil query id, the agent's own parcels.
async fn holdings(
    a: &mut Session,
    products: &[LandProduct],
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let id = Uuid::nil();
    let leg = ask(
        a,
        Command::PlacesQuery {
            query_id: QueryId::from(id),
            transaction_id: TransactionId::from(Uuid::nil()),
            query_text: String::new(),
            flags: DirFindFlags::AGENT_OWNED,
            category: ParcelCategory::Any,
            sim_name: String::new(),
        },
        id,
        Kind::Holdings,
    )
    .await?;
    metrics.set("holdings_own", answer_text(&leg, id, Kind::Holdings));
    metrics.set("holdings_own_products", sku_text(&leg.rows(id), products));
    Ok(())
}

/// Leg 9: the land products, raw and as the client reads them. Returns the
/// client's list, for the land legs to name their rows by.
async fn product_info(
    a: &mut Session,
    metrics: &mut Metrics,
) -> Result<Vec<LandProduct>, TestFailure> {
    match cap(a, PRODUCT_INFO_REQUEST).await {
        None => {
            metrics.set("product_info_answer", NO_CAPABILITY);
            metrics.set("product_info_skus", NO_CAPABILITY);
            metrics.set("product_info_list", NO_CAPABILITY);
        }
        Some(url) => {
            let response = http_client()?
                .get(url)
                .header("Accept", "application/llsd+xml")
                .send()
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("a capability request failed: {error}"))
                })?;
            let status = response.status().as_u16();
            let llsd = parse_llsd_xml(&response.text().await.unwrap_or_default()).ok();
            let entries: Vec<Llsd> = match llsd.as_ref() {
                Some(Llsd::Array(entries)) => entries.clone(),
                _ => Vec::new(),
            };
            metrics.set(
                "product_info_answer",
                format!(
                    "{status}, {}, {} entr(ies), an entry {}",
                    llsd.as_ref().map_or_else(
                        || "no LLSD".to_owned(),
                        |llsd| match llsd {
                            Llsd::Array(_) => "an array".to_owned(),
                            other => shape_of(other),
                        }
                    ),
                    entries.len(),
                    entries
                        .first()
                        .map_or_else(|| "absent".to_owned(), shape_of)
                ),
            );
            let mut skus: Vec<String> = entries
                .iter()
                .filter_map(|entry| match entry.get("sku") {
                    Some(Llsd::String(sku)) => Some(sku.clone()),
                    _ => None,
                })
                .collect();
            skus.sort();
            metrics.set("product_info_skus", skus.join(","));
            // The whole list, for the fake grid to serve; not held.
            let mut listed: Vec<String> = entries
                .iter()
                .map(|entry| {
                    let text = |key: &str| match entry.get(key) {
                        Some(Llsd::String(text)) => text.clone(),
                        _ => String::new(),
                    };
                    format!("{}={}|{}", text("sku"), text("name"), text("description"))
                })
                .collect();
            listed.sort();
            metrics.set("product_info_list", listed.join(" ; "));
        }
    }
    let leg = ask_until(a, Command::RequestLandProducts, Kind::Land, |got| {
        !got.products.is_empty()
    })
    .await?;
    let products = leg.got.products.first().cloned().unwrap_or_default();
    metrics.set(
        "product_info_client",
        match leg.got.products.first() {
            None => "nothing".to_owned(),
            Some(list) => format!(
                "{} product(s), {} with a description",
                list.len(),
                list.iter()
                    .filter(|product| !product.description.is_empty())
                    .count()
            ),
        },
    );
    Ok(products)
}

#[cfg(test)]
mod tests {
    use super::{Row, count_text, nil_text, relation_text, search_name};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;

    /// A row of `id`.
    fn row(id: u128) -> Row {
        super::plain(Uuid::from_u128(id))
    }

    /// Counts read against the reference's page of 100.
    #[test]
    fn counts_read_against_the_page() {
        assert_eq!(count_text(0), "no rows");
        assert_eq!(count_text(1), "1 row");
        assert_eq!(count_text(42), "fewer than a page of rows");
        assert_eq!(count_text(100), "a page of rows");
        assert_eq!(count_text(101), "a page and one more row");
        assert_eq!(count_text(150), "more than a page and one rows");
    }

    /// The nil rows are placed.
    #[test]
    fn nil_rows_are_placed() {
        let (nil, one, two) = (row(0), row(1), row(2));
        assert_eq!(nil_text(&[&nil]), "the one row nil");
        assert_eq!(nil_text(&[&one, &two, &nil]), "the last row nil");
        assert_eq!(nil_text(&[&one, &nil, &two]), "one nil row amid");
        assert_eq!(nil_text(&[&one, &two]), "no nil row");
        assert_eq!(nil_text(&[&nil, &nil]), "every row nil");
    }

    /// Two answers' real rows are related.
    #[test]
    fn answers_are_related_by_their_real_rows() {
        let (nil, one, two, three) = (row(0), row(1), row(2), row(3));
        assert_eq!(relation_text(&[&nil], &[&nil]), "neither has a row");
        assert_eq!(relation_text(&[&one], &[&nil]), "none of its own");
        assert_eq!(relation_text(&[&one, &two], &[&two, &one]), "the same rows");
        assert_eq!(relation_text(&[&one], &[&two]), "other rows only");
        assert_eq!(
            relation_text(&[&one, &two], &[&two]),
            "some of the same rows"
        );
        assert_eq!(
            relation_text(&[&one], &[&one, &two]),
            "every one of the rows and others"
        );
        assert_eq!(
            relation_text(&[&one, &two], &[&two, &three]),
            "one of the same rows and others"
        );
        let four = row(4);
        assert_eq!(
            relation_text(&[&one, &two, &three], &[&two, &three, &four]),
            "several of the same rows and others"
        );
    }

    /// A Resident is searched for by the first name alone.
    #[test]
    fn a_resident_is_searched_by_first_name() {
        assert_eq!(search_name("Example Resident"), "Example");
        assert_eq!(search_name("Avatar Tester"), "Avatar Tester");
    }
}
