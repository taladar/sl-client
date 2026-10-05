//! Request a parcel's dwell (traffic) value and its grid-wide info listing.
//!
//! Two distinct request/reply pairs answer "tell me about this parcel", and this
//! case exercises both against the parcel at the region centre:
//!
//! 1. **Dwell** — a UDP `ParcelDwellRequest` for the parcel's *region-local* id
//!    ([`Command::RequestParcelDwell`]) answered by a UDP `ParcelDwellReply`
//!    ([`Event::ParcelDwell`]). The request takes a [`ScopedParcelId`]: the
//!    region-local id learned from a `ParcelProperties` reply, paired with the
//!    root circuit's identity (so a stale id from a previous circuit fails to
//!    resolve rather than hitting the wrong parcel).
//! 2. **Info listing** — the condensed "places/search" record a viewer shows for
//!    a parcel *id*. The id is grid-wide, not region-local, so it is first
//!    resolved from the region-centre location through the `RemoteParcelRequest`
//!    **capability** ([`Command::RequestRemoteParcelId`] →
//!    [`Event::RemoteParcelId`]); that id then feeds a UDP `ParcelInfoRequest`
//!    ([`Command::RequestParcelInfo`]) answered by a `ParcelInfoReply`
//!    ([`Event::ParcelDetails`]). The listing carries its own dwell field, so the
//!    case records both the dedicated dwell reply and the info reply's dwell.
//!
//! The flow:
//!
//! 1. Wait for the region to become active.
//! 2. Send a `ParcelPropertiesRequest` for a square at the region centre to learn
//!    the parcel's region-local id.
//! 3. Request the parcel's dwell by that region-local id (scoped to the root
//!    circuit) and await the matching `ParcelDwell`.
//! 4. Resolve the region-centre location to a grid-wide parcel id via the
//!    `RemoteParcelRequest` capability.
//! 5. Request that parcel's info listing and await the `ParcelDetails` whose
//!    echoed id matches, then hold the listing to the parcel record it
//!    condenses: its name, its area, and its flags byte.
//! 6. Where the agent may sell the parcel, put it up for sale, read the listing
//!    again, and take it off the market.
//! 7. Where the grid answers the searches, read the listings of parcels a land
//!    search and a places search return, and compare each one's rating bits to
//!    its region's rating on the world map.
//!
//! # The listing's flags byte
//!
//! Steps 5 to 7 exist for one byte. A listing's `Flags` is not the parcel-flags
//! field cut down: it packs the region's rating (`0x01` moderate, `0x02`
//! adult), group ownership (`0x04`) and for-sale (`0x80`), and its `0x04` is the
//! value the *parcel* flags use for for-sale — which is how this client used to
//! read it, pricing every group-owned parcel and none that was for sale
//! (`protocol-parcel-info-reply-flags-misread`). The reference viewer reads only
//! the rating and the ownership out of it, so only a grid could say what the
//! rest means.
//!
//! Three sources of truth are compared against the byte, each where it exists:
//!
//! - the **parcel's own record** (step 5) — a group owner and a for-sale parcel
//!   are both visible in the `ParcelProperties` of the same parcel;
//! - an **edit** (step 6) — needs the right to sell the land, so it runs
//!   wherever the agent has it: as OpenSim's `estate-owner`, and on the fake
//!   grids, which enforce none. The aditi test avatars own no land; there the
//!   step is skipped and step 7 measures a for-sale listing instead;
//! - the **search rows** (step 7) — a land-search row says for-sale and at what
//!   price. Only Second Life answers these searches (and the land search over
//!   the event queue, never UDP), so this is the aditi half of step 6.
//!
//! The sale-price field is filled whether or not the parcel is for sale — on
//! aditi a parcel off the market still carries the last price it was set to —
//! so a listing has a price only when the for-sale bit says so.
//!
//! `1av`, every grid; run as `estate-owner` on OpenSim for step 6 (any other
//! avatar there records the run `partial`). On OpenSim's Default Region the
//! single region-wide parcel answers all three requests; the dwell is 0 on a
//! fresh region (no accumulated traffic) but the reply still arrives (the
//! `DefaultDwellModule` is enabled by default). Aditi reports a dwell of 0 for
//! every parcel sampled, busy or not. The fake grid's catalogue region carries
//! one region-wide parcel with a grid-wide listing and a deliberately non-zero
//! dwell, so the replies are asserted offline.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, DirFindFlags, Event, LandSearchType, LindenAmount, Maturity, ParcelCategory,
    ParcelDetails, ParcelFlags, ParcelInfo, ParcelKey, ParcelListingFlags, QueryId,
    RegionCoordinates, RegionLocalParcelId, RegionName, ScopedParcelId, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// The western/southern edge of the queried square, in region metres — a 4×4 m
/// square centred on the region centre (128, 128), so the reply describes the
/// parcel at the middle of the region.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres (see
/// [`SQUARE_WEST_SOUTH`]).
const SQUARE_EAST_NORTH: f32 = 128.0;

/// The region-centre location whose parcel the `RemoteParcelRequest` resolves.
/// The z is irrelevant — the grid keys the lookup on the x/y column.
const REGION_CENTRE: f32 = 128.0;

/// A distinctive sequence id, echoed back in the `ParcelProperties` reply so the
/// awaited reply is *our* query's answer and not an unsolicited on-entry one.
/// Distinct from the `parcel-properties` case's id so the two never alias.
const SEQUENCE_ID: i32 = 5151;

/// The price the sale step asks. Distinctive, so the listing that carries it is
/// the one this step produced and not a price the parcel already had.
const SALE_PRICE: LindenAmount = LindenAmount(4242);

/// How long a search or a map lookup is given before the grid is recorded as
/// not answering it. Short, because on OpenSim (which answers neither search)
/// and for a region the map does not know, the silence *is* the answer.
const SEARCH_WINDOW: Duration = Duration::from_secs(8);

/// How long a listing is left alone before it is read again, when the first
/// read after an edit still shows the parcel as it was. OpenSim answers a
/// listing out of a cache whose entry lives 30 s past its **last read** and is
/// swept every 10 s (`LandManagementModule.m_parcelInfoCache`), so asking again
/// sooner keeps the stale answer alive; only silence lets it expire.
const LISTING_CACHE_WAIT: Duration = Duration::from_secs(45);

/// How many of a search's rows have their listing read. Enough to meet more
/// than one rating and both kinds of owner; few enough to be a handful of
/// requests rather than a crawl of somebody's search index.
const ROWS_SAMPLED: usize = 8;

/// The rating bits (`Flags & 0x03`) of a listing whose region is adult: Second
/// Life sets the moderate bit beside the adult one, OpenSim the adult bit alone.
/// The OpenSim half is read from `Util.ConvertAccessLevelToMaturity` — the local
/// grid has no adult region to measure — and is held only where a run meets one.
const ADULT_RATING_BITS: Measured<u8> = Measured {
    second_life: 0x03,
    opensim: 0x02,
    source: "parcel-info-dwell on aditi; OpenSim from source (2026-10-05, \
             book/src/gridspec/land.md)",
};

/// Whether a listing read straight after an edit still shows the parcel as it
/// was on `grid`. The local OpenSim's does, for as long as it keeps being asked
/// for (see [`LISTING_CACHE_WAIT`]; measured as its estate owner, 2026-10-05,
/// `book/src/gridspec/land.md`).
///
/// This is deliberately not a [`Measured`] pair. Second Life's answer is
/// unmeasured — the test avatars own no land on aditi, where the step that
/// observes this is skipped — and the fake grid does **not** imitate OpenSim's
/// cache: it would add a minute and a half of waiting to every offline run for
/// a lag no viewer can act on. Both fake flavours answer from the live record.
const fn listing_lags_an_edit(grid: Grid) -> bool {
    matches!(grid, Grid::Opensim)
}

/// The bits of a listing's flags byte that say how its region is rated.
const RATING_BITS: u8 = ParcelListingFlags::MATURE.bits() | ParcelListingFlags::ADULT.bits();

/// Requests a parcel's dwell and its grid-wide info listing, and holds the
/// listing's flags byte to the parcel record, an edit and the searches.
#[derive(Debug)]
pub struct ParcelInfoDwell;

impl GridTest for ParcelInfoDwell {
    fn name(&self) -> &'static str {
        "parcel-info-dwell"
    }

    fn description(&self) -> &'static str {
        "Request a parcel's dwell and its grid-wide info listing"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::FakeSl, Grid::FakeOpensim, Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;
            let region_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login reported no region handle".to_owned())
            })?;

            // 1. Learn the parcel's region-local id from a ParcelProperties reply
            //    (the dwell request is keyed on the region-local id, not the
            //    grid-wide one).
            session
                .send(Command::RequestParcelProperties {
                    west: SQUARE_WEST_SOUTH,
                    south: SQUARE_WEST_SOUTH,
                    east: SQUARE_EAST_NORTH,
                    north: SQUARE_EAST_NORTH,
                    sequence_id: SEQUENCE_ID,
                    snap_selection: false,
                })
                .await?;
            let parcel: ParcelInfo = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                        Some((**parcel).clone())
                    }
                    _ => None,
                })
                .await?;
            check(
                parcel.request_result.has_data(),
                &format!(
                    "parcel query returned no data (request_result: {:?})",
                    parcel.request_result
                ),
            )?;
            let local_id = parcel.local_id;

            // 2. Request the parcel's dwell by its region-local id, scoped to the
            //    root circuit.
            let dwell_start = Instant::now();
            session
                .send(Command::RequestParcelDwell {
                    local_id: ScopedParcelId::new(circuit, local_id),
                })
                .await?;
            let (dwell_local_id, dwell_parcel_id, dwell) = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ParcelDwell {
                        local_id,
                        parcel_id,
                        dwell,
                    } => Some((*local_id, *parcel_id, *dwell)),
                    _ => None,
                })
                .await?;
            let dwell_elapsed = dwell_start.elapsed().as_secs_f64();
            check_eq("dwell reply local id", &dwell_local_id.id(), &local_id)?;

            // 3. Resolve the region-centre location to a grid-wide parcel id via
            //    the RemoteParcelRequest capability.
            let remote_start = Instant::now();
            session
                .send(Command::RequestRemoteParcelId {
                    location: RegionCoordinates::new(REGION_CENTRE, REGION_CENTRE, 0.0),
                    region_id: Uuid::nil(),
                    region_handle,
                })
                .await?;
            // The answer carries the question, so this accepts only the reply to
            // the location just asked about rather than whichever resolve
            // happens to land first.
            let parcel_key = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::RemoteParcelId {
                        parcel_id,
                        location,
                        region_id: _,
                        region_handle: answered,
                    } if *location == RegionCoordinates::new(REGION_CENTRE, REGION_CENTRE, 0.0)
                        && *answered == region_handle =>
                    {
                        Some(*parcel_id)
                    }
                    _ => None,
                })
                .await?;
            let remote_elapsed = remote_start.elapsed().as_secs_f64();
            check(
                !parcel_key.uuid().is_nil(),
                "RemoteParcelRequest resolved the region centre to a nil parcel id",
            )?;

            // 4. Request that parcel's info listing by its grid-wide id.
            let info_start = Instant::now();
            let details = request_listing(session, parcel_key, REPLY_TIMEOUT)
                .await?
                .ok_or_else(|| {
                    TestFailure::Assertion(
                        "no ParcelInfoReply for the parcel under the agent".to_owned(),
                    )
                })?;
            let info_elapsed = info_start.elapsed().as_secs_f64();
            check_eq("info listing parcel id", &details.parcel_id, &parcel_key)?;
            check(
                details.sim_name.is_some(),
                "parcel info listing carried no region name",
            )?;

            // 5. The listing against the record it condenses. Both come out of
            //    the grid's one land record, so they have to agree.
            //
            //    IF ONE OF THESE FAILS ON THE LIVE OPENSIM, suspect its listing
            //    cache before the client: `LandManagementModule` answers a
            //    `ParcelInfoRequest` from `m_parcelInfoCache`, whose entry
            //    lives 30 s past its *last read* and is swept every 10 s. A
            //    listing read within ~40 s of anything that changed the
            //    parcel — this case's own step 6 in a run that died before
            //    restoring, `parcel-edit` renaming it, a viewer saving About
            //    Land — still shows the parcel as it was, and re-running
            //    straight away keeps that answer alive. Leave the grid alone
            //    for `LISTING_CACHE_WAIT` and run again; the `ParcelProperties`
            //    side is never cached. (`book/src/gridspec/land.md`, § Parcel
            //    info.)
            check_eq("listing name", &details.name, &parcel.name)?;
            check_eq("listing actual area", &details.actual_area, &parcel.area)?;
            check_eq("listing owner id", &details.owner_id, &parcel.owner.uuid())?;
            check_eq(
                "listing group-owned bit",
                &details.flags.is_group_owned(),
                &parcel.owner.is_group(),
            )?;
            check_eq(
                "listing for-sale bit",
                &details.flags.is_for_sale(),
                &parcel.flags().contains(ParcelFlags::FOR_SALE),
            )?;
            check_eq(
                "listing sale price",
                &details.sale_price,
                &parcel.sale_price,
            )?;
            let own_region_rating = match details.sim_name.as_ref() {
                Some(name) => region_rating(session, name).await?,
                None => None,
            };
            if let Some(rating) = own_region_rating {
                check_rating(grid, "the parcel under the agent", details.flags, rating)?;
            }

            // 6. Put the parcel up for sale and read the listing again — where
            //    the agent may. The aditi avatars own no land, so step 7 is
            //    where a for-sale listing is measured there.
            let sale = if grid == Grid::Aditi {
                SaleStep::NoRights
            } else {
                sale_step(session, &parcel, parcel_key).await?
            };

            // 7. Listings of parcels the searches return, where the grid
            //    answers the searches.
            let land = sample_land_search(session).await?;
            let places = sample_places_search(session).await?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("parcel_dwell"), dwell_elapsed);
            metrics.set_timing(&secs_metric("remote_parcel_id"), remote_elapsed);
            metrics.set_timing(&secs_metric("parcel_info"), info_elapsed);
            metrics.set("local_id", i64::from(local_id.0));
            metrics.set("dwell", f64::from(dwell));
            metrics.set("dwell_parcel_id", dwell_parcel_id.to_string());
            metrics.set("parcel_id", parcel_key.to_string());
            metrics.set("info_dwell", f64::from(details.dwell));
            metrics.set("listing", describe(&details, own_region_rating));
            metrics.set("listing_flags", i64::from(details.flags.bits()));
            metrics.set(
                "listing_has_snapshot",
                details.snapshot_id.is_some() == parcel.snapshot_id.is_some(),
            );
            metrics.set("auction_id", i64::from(details.auction_id));
            match &sale {
                SaleStep::NoRights => metrics.set("sale_step", "skipped: no land rights"),
                SaleStep::Refused => metrics.set("sale_step", "the edit was not applied"),
                SaleStep::Measured {
                    for_sale,
                    restored,
                    lagged,
                } => {
                    metrics.set("sale_step", "measured");
                    metrics.set(
                        "listing_follows_edit",
                        if *lagged {
                            "only once it had not been asked for for a while"
                        } else {
                            "at once"
                        },
                    );
                    metrics.set("for_sale_listing", describe(for_sale, own_region_rating));
                    metrics.set("restored_listing", describe(restored, own_region_rating));
                }
            }
            land.record(metrics, "land");
            places.record(metrics, "places");
            metrics.set("actual_area", i64::from(details.actual_area.0));
            metrics.set("billable_area", i64::from(details.billable_area.0));
            metrics.set("owner_id", details.owner_id.to_string());
            metrics.set("parcel_name", details.name);
            metrics.set(
                "region_name",
                details
                    .sim_name
                    .map(|name| name.to_string())
                    .unwrap_or_default(),
            );
            // The verdicts, after the record: a failing run still documents
            // what each listing said.
            if let SaleStep::Measured {
                for_sale,
                restored,
                lagged,
            } = &sale
            {
                check_eq(
                    "whether the listing lagged the edit",
                    lagged,
                    &listing_lags_an_edit(grid),
                )?;
                check(
                    for_sale.flags.is_for_sale(),
                    "the listing of a parcel put up for sale lacks the for-sale bit",
                )?;
                check_eq(
                    "for-sale listing price",
                    &for_sale.sale_price,
                    &Some(SALE_PRICE),
                )?;
                check_eq(
                    "for-sale listing's other flags",
                    &for_sale.flags.with_for_sale(false),
                    &details.flags.with_for_sale(false),
                )?;
                check_eq("restored listing flags", &restored.flags, &details.flags)?;
                check_eq(
                    "restored listing price",
                    &restored.sale_price,
                    &details.sale_price,
                )?;
            }
            land.verdict(grid, "land search")?;
            places.verdict(grid, "places search")?;
            if land.answered {
                check(
                    land.for_sale_matching > 0,
                    "no sampled land-search row's listing was for sale at the row's price",
                )?;
            }
            if matches!(sale, SaleStep::Refused) {
                ctx.mark_partial(
                    "the agent may not sell the parcel under it, so the for-sale listing was \
                     not measured (run as estate-owner)",
                );
            }
            Ok(())
        })
    }
}

/// What step 6 came to.
#[derive(Debug)]
enum SaleStep {
    /// Not attempted: the agent has no right to sell land on this grid.
    NoRights,
    /// Attempted, and the region never showed the parcel for sale.
    Refused,
    /// The listing while the parcel was for sale, and after it was taken off
    /// the market again.
    Measured {
        /// The listing while for sale.
        for_sale: Box<ParcelDetails>,
        /// The listing after the original record was saved back.
        restored: Box<ParcelDetails>,
        /// Whether either listing still showed the parcel as it was before
        /// the edit when first read, and had to be left alone to catch up.
        lagged: bool,
    },
}

/// Asks for one parcel's listing and waits `window` for it. `None` is a grid
/// that did not answer — a parcel id a search returned may be one its
/// simulator no longer knows.
async fn request_listing(
    session: &mut Session,
    parcel_id: ParcelKey,
    window: Duration,
) -> Result<Option<ParcelDetails>, TestFailure> {
    session
        .send(Command::RequestParcelInfo { parcel_id })
        .await?;
    match session
        .wait_for(window, |event| match event {
            Event::ParcelDetails(details) if details.parcel_id == parcel_id => {
                Some(details.clone())
            }
            _ => None,
        })
        .await
    {
        Ok(details) => Ok(Some(details)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// The rating the world map gives the region called `name`, or `None` when the
/// map does not answer for it. A name search matches by prefix, so only the
/// block whose name is exactly `name` counts.
async fn region_rating(
    session: &mut Session,
    name: &RegionName,
) -> Result<Option<Maturity>, TestFailure> {
    session
        .send(Command::RequestMapByName {
            name: name.to_string(),
        })
        .await?;
    match session
        .wait_for(SEARCH_WINDOW, |event| match event {
            Event::MapBlock(block) if block.name.as_ref() == Some(name) => Some(block.maturity),
            _ => None,
        })
        .await
    {
        Ok(Maturity::Unknown) | Err(TestFailure::Timeout(_)) => Ok(None),
        Ok(rating) => Ok(Some(rating)),
        Err(other) => Err(other),
    }
}

/// Holds a listing's rating bits to its region's rating: the decoded rating is
/// the region's on either grid, and an adult region's bits are the ones this
/// grid was measured packing.
fn check_rating(
    grid: Grid,
    what: &str,
    flags: ParcelListingFlags,
    region: Maturity,
) -> Result<(), TestFailure> {
    check_eq(
        &format!("rating of the listing of {what}"),
        &flags.maturity(),
        &region,
    )?;
    if region == Maturity::Adult {
        ADULT_RATING_BITS.check(
            &format!("rating bits of the listing of {what}"),
            grid,
            &(flags.bits() & RATING_BITS),
        )?;
    }
    Ok(())
}

/// One listing as a record line: the raw byte, what it decodes to, and what
/// the region's rating was found to be.
fn describe(details: &ParcelDetails, region: Option<Maturity>) -> String {
    format!(
        "flags={:#04x} rating={:?} region_rating={} group_owned={} for_sale={} price={} \
         area={}/{} snapshot={} dwell={} auction={}",
        details.flags.bits(),
        details.flags.maturity(),
        region.map_or_else(|| "unknown".to_owned(), |rating| format!("{rating:?}")),
        details.flags.is_group_owned(),
        details.flags.is_for_sale(),
        details
            .sale_price
            .as_ref()
            .map_or_else(|| "none".to_owned(), |price| price.0.to_string()),
        details.actual_area.0,
        details.billable_area.0,
        details.snapshot_id.is_some(),
        details.dwell,
        details.auction_id,
    )
}

/// Step 6: save the parcel for sale, read its listing, save the original
/// record back and read the listing once more. The parcel is restored before
/// anything is judged, so a wrong listing does not leave land on the market.
async fn sale_step(
    session: &mut Session,
    original: &ParcelInfo,
    parcel_key: ParcelKey,
) -> Result<SaleStep, TestFailure> {
    let local_id = original.local_id;
    let mut selling = original.to_update();
    selling.parcel_flags = selling.parcel_flags.union(ParcelFlags::FOR_SALE);
    selling.sale_price = Some(SALE_PRICE);
    session
        .send(Command::UpdateParcel(Box::new(selling)))
        .await?;
    if !await_sale_state(session, local_id, Some(&SALE_PRICE)).await? {
        return Ok(SaleStep::Refused);
    }
    let for_sale = settled_listing(session, parcel_key, Some(&SALE_PRICE)).await;

    // The parcel goes back before the first read is judged, so a listing that
    // could not be read does not leave land on the market.
    session
        .send(Command::UpdateParcel(Box::new(original.to_update())))
        .await?;
    check(
        await_sale_state(session, local_id, original.sale_price.as_ref()).await?,
        "the parcel did not return to its original sale state",
    )?;
    let (for_sale, for_sale_lagged) = for_sale?;
    let (restored, restored_lagged) =
        settled_listing(session, parcel_key, original.sale_price.as_ref()).await?;
    Ok(SaleStep::Measured {
        for_sale: Box::new(for_sale),
        restored: Box::new(restored),
        lagged: for_sale_lagged || restored_lagged,
    })
}

/// Reads a parcel's listing after an edit that should have left it with the
/// sale price `expected`. A listing that still shows the old state is left
/// alone for [`LISTING_CACHE_WAIT`] and read once more; the flag says whether
/// that was needed. The second read is returned whatever it shows — judging it
/// is the caller's.
async fn settled_listing(
    session: &mut Session,
    parcel_key: ParcelKey,
    expected: Option<&LindenAmount>,
) -> Result<(ParcelDetails, bool), TestFailure> {
    let missing = || {
        TestFailure::Assertion(
            "no ParcelInfoReply for the parcel after its sale state changed".to_owned(),
        )
    };
    let first = request_listing(session, parcel_key, REPLY_TIMEOUT)
        .await?
        .ok_or_else(missing)?;
    if first.sale_price.as_ref() == expected {
        return Ok((first, false));
    }
    // Wait without asking: the events are drained, none is awaited.
    match session
        .wait_for(LISTING_CACHE_WAIT, |_event| None::<()>)
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => {}
        Err(other) => return Err(other),
    }
    let second = request_listing(session, parcel_key, REPLY_TIMEOUT)
        .await?
        .ok_or_else(missing)?;
    Ok((second, true))
}

/// Waits for the region to push the parcel with the sale price `expected`
/// (`None`: not for sale). `false` is a region that never did — an edit it
/// refused pushes the parcel back unchanged, or pushes nothing.
async fn await_sale_state(
    session: &mut Session,
    local_id: RegionLocalParcelId,
    expected: Option<&LindenAmount>,
) -> Result<bool, TestFailure> {
    match session
        .wait_for(SEARCH_WINDOW, |event| match event {
            Event::ParcelProperties(parcel)
                if parcel.local_id == local_id && parcel.sale_price.as_ref() == expected =>
            {
                Some(())
            }
            _ => None,
        })
        .await
    {
        Ok(()) => Ok(true),
        Err(TestFailure::Timeout(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

/// One search row whose listing is read: the parcel, and — for a land-search
/// row — the price the row says it is for sale at.
#[derive(Debug)]
struct SearchRow {
    /// The parcel the row names.
    parcel_id: ParcelKey,
    /// The row's asking price, where the search states one.
    price: Option<LindenAmount>,
}

/// What reading a search's listings found.
#[derive(Debug, Default)]
struct SearchSample {
    /// Whether the grid answered the search at all.
    answered: bool,
    /// How many rows the first reply carried.
    rows: usize,
    /// One record line per listing read.
    listings: Vec<String>,
    /// Listings that were for sale at the price their search row stated.
    for_sale_matching: usize,
    /// Each sampled listing's flags and its region's rating, where the map
    /// knew the region.
    ratings: Vec<(ParcelListingFlags, Maturity)>,
}

impl SearchSample {
    /// Writes the sample under `prefix`.
    fn record(&self, metrics: &mut crate::metrics::Metrics, prefix: &str) {
        metrics.set(&format!("{prefix}_search_answered"), self.answered);
        metrics.set(
            &format!("{prefix}_search_rows"),
            i64::try_from(self.rows).unwrap_or(i64::MAX),
        );
        for (index, line) in self.listings.iter().enumerate() {
            metrics.set(&format!("{prefix}_listing_{index}"), line.clone());
        }
        metrics.set(
            &format!("{prefix}_listings_for_sale_at_row_price"),
            i64::try_from(self.for_sale_matching).unwrap_or(i64::MAX),
        );
        let adult = self
            .ratings
            .iter()
            .filter(|(_, region)| *region == Maturity::Adult)
            .count();
        metrics.set(
            &format!("{prefix}_listings_in_adult_regions"),
            i64::try_from(adult).unwrap_or(i64::MAX),
        );
    }

    /// Holds every sampled listing's rating bits to its region's rating.
    fn verdict(&self, grid: Grid, what: &str) -> Result<(), TestFailure> {
        for (flags, region) in &self.ratings {
            check_rating(grid, &format!("a {what} row"), *flags, *region)?;
        }
        Ok(())
    }
}

/// Reads the listings of the first [`ROWS_SAMPLED`] of `rows`, looking each
/// one's region up on the map once.
async fn sample_rows(
    session: &mut Session,
    rows: Vec<SearchRow>,
) -> Result<SearchSample, TestFailure> {
    let mut sample = SearchSample {
        answered: true,
        rows: rows.len(),
        ..SearchSample::default()
    };
    let mut ratings: BTreeMap<String, Option<Maturity>> = BTreeMap::new();
    for row in rows.into_iter().take(ROWS_SAMPLED) {
        let Some(details) = request_listing(session, row.parcel_id, SEARCH_WINDOW).await? else {
            sample.listings.push("no reply".to_owned());
            continue;
        };
        let rating = match details.sim_name.as_ref() {
            Some(name) => {
                if let Some(known) = ratings.get(name.as_ref()) {
                    *known
                } else {
                    let found = region_rating(session, name).await?;
                    let _previous = ratings.insert(name.to_string(), found);
                    found
                }
            }
            None => None,
        };
        if let Some(rating) = rating {
            sample.ratings.push((details.flags, rating));
        }
        if row.price.is_some() && details.sale_price == row.price {
            sample.for_sale_matching = sample.for_sale_matching.saturating_add(1);
        }
        sample.listings.push(describe(&details, rating));
    }
    Ok(sample)
}

/// The maturity-inclusion bits a search needs to return parcels of every
/// rating.
const EVERY_RATING: DirFindFlags = DirFindFlags::INC_PG
    .union(DirFindFlags::INC_MATURE)
    .union(DirFindFlags::INC_ADULT);

/// Step 7, land: the land-for-sale search, dearest first, and the listings of
/// its first rows. Second Life answers over the event queue; a grid that does
/// not answer is recorded as such.
async fn sample_land_search(session: &mut Session) -> Result<SearchSample, TestFailure> {
    let query_id = QueryId::from(Uuid::new_v4());
    session
        .send(Command::DirLandQuery {
            query_id,
            flags: EVERY_RATING
                .union(DirFindFlags::FOR_SALE)
                .union(DirFindFlags::PRICE_SORT),
            search_type: LandSearchType::ALL,
            price: 0,
            area: 0,
            query_start: 0,
        })
        .await?;
    let rows = match session
        .wait_for(SEARCH_WINDOW, |event| match event {
            Event::DirLandReply {
                query_id: answered,
                results,
            } if *answered == query_id.get() => Some(
                results
                    .iter()
                    .filter(|row| row.for_sale)
                    .map(|row| SearchRow {
                        parcel_id: row.parcel_id,
                        price: row.sale_price.clone(),
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .await
    {
        Ok(rows) => rows,
        Err(TestFailure::Timeout(_)) => return Ok(SearchSample::default()),
        Err(other) => return Err(other),
    };
    sample_rows(session, rows).await
}

/// The text of the places search: a word common enough in parcel names that a
/// grid with a search index at all has rows for it.
const PLACES_QUERY: &str = "linden";

/// Step 7, places: a places search of every category and rating, and the
/// listings of its first rows — parcels that are, mostly, not for sale.
async fn sample_places_search(session: &mut Session) -> Result<SearchSample, TestFailure> {
    let query_id = QueryId::from(Uuid::new_v4());
    session
        .send(Command::DirPlacesQuery {
            query_id,
            query_text: PLACES_QUERY.to_owned(),
            flags: EVERY_RATING.union(DirFindFlags::DWELL_SORT),
            category: ParcelCategory::None,
            sim_name: String::new(),
            query_start: 0,
        })
        .await?;
    let rows = match session
        .wait_for(SEARCH_WINDOW, |event| match event {
            Event::DirPlacesReply {
                query_id: answered,
                results,
                ..
            } if *answered == query_id.get() => Some(
                results
                    .iter()
                    .map(|row| SearchRow {
                        parcel_id: row.parcel_id,
                        price: None,
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .await
    {
        Ok(rows) => rows,
        Err(TestFailure::Timeout(_)) => return Ok(SearchSample::default()),
        Err(other) => return Err(other),
    };
    sample_rows(session, rows).await
}
