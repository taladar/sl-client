//! Exercise the SLM DirectDelivery JSON transport (merchant probe +
//! listings fetch).
//!
//! The Second Life Marketplace (SLM) listing-management service behind
//! the region's `DirectDelivery` capability is the protocol surface's
//! only plain-JSON REST API (Firestorm `llmarketplacefunctions.cpp`).
//! OpenSim has no `DirectDelivery` capability at all, so this is the
//! suite's first **aditi-only** case. Without a merchant store on the
//! test avatar only the transport level is reachable: the case drives
//! `GET /merchant` (whose payload is the HTTP status code — 404 means
//! "not a merchant") and `GET /listings`, records what the service
//! answers, and always marks the run partial — the mutation routes
//! (`POST /listings`, `PUT /listing/<id>`,
//! `PUT /associate_inventory/<id>`, `DELETE /listing/<id>`) need a
//! real merchant store and stay above this case's conformance
//! ceiling. The JSON request/response codec itself is covered by
//! `sl-marketplace`'s and `sl-proto`'s unit tests. `[aditi] 1av`.
//!
//! Whether aditi even grants the capability to a storeless avatar was
//! unknown when this case was written; a region that does not grant it
//! also passes as partial (recording the absence) rather than failing.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, MarketplaceOperation, MerchantStatus};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, count_metric};

/// How long to poll for the `DirectDelivery` capability after region
/// arrival (the seed-capability fetch races region arrival, so the map
/// may fill in shortly afterwards).
const CAP_WAIT: Duration = Duration::from_secs(20);

/// How long to wait for each SLM reply. The service is an external web
/// service reached through the capability URL, so give it a web-ish
/// budget rather than a UDP-ish one.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Exercises the SLM DirectDelivery transport ceiling reachable
/// without a merchant store.
#[derive(Debug)]
pub struct MarketplaceDirectDelivery;

impl GridTest for MarketplaceDirectDelivery {
    fn name(&self) -> &'static str {
        "marketplace-direct-delivery"
    }

    fn description(&self) -> &'static str {
        "Exercise the SLM DirectDelivery JSON transport (merchant probe + listings)"
    }

    fn grids(&self) -> &'static [Grid] {
        // SL-only: OpenSim serves no DirectDelivery capability, so even a
        // partial OpenSim run would only re-observe the missing-cap event.
        &[Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;

            // The seed-capability fetch races region arrival — poll briefly
            // before concluding the region does not grant the capability.
            let cap_wait_started = Instant::now();
            while ctx.primary().cap("DirectDelivery").is_none()
                && cap_wait_started.elapsed() < CAP_WAIT
            {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let has_cap = ctx.primary().cap("DirectDelivery").is_some();
            ctx.metrics().set("has_direct_delivery_cap", has_cap);
            if !has_cap {
                ctx.mark_partial(
                    "region grants no DirectDelivery capability to this \
                     avatar; SLM transport unreachable",
                );
                return Ok(());
            }

            // The merchant probe: the HTTP status code is the payload. A
            // storeless test avatar should be a proper non-merchant (404);
            // any defined answer proves the transport, while a connection
            // failure means the service was not reached.
            ctx.primary()
                .send(Command::MarketplaceMerchantStatus)
                .await?;
            let merchant_status = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::MarketplaceMerchantStatus(status) => Some(status.clone()),
                    _ => None,
                })
                .await?;
            ctx.metrics()
                .set("merchant_status", format!("{merchant_status}"));
            check(
                !matches!(merchant_status, MerchantStatus::ConnectionFailure { .. }),
                "expected a defined merchant status from GET /merchant, \
                 got a connection failure",
            )?;

            // GET /listings for a non-merchant is undocumented: a typed
            // error and an empty listings set are both conforming — which
            // one aditi answers is itself the datum being recorded. Only a
            // transport failure (or timeout) fails the case.
            ctx.primary().send(Command::MarketplaceListings).await?;
            let listings_outcome = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::MarketplaceListings(listings) => Some(Ok(listings.len())),
                    Event::MarketplaceError {
                        operation: MarketplaceOperation::GetListings,
                        error,
                    } => Some(Err(error.clone())),
                    _ => None,
                })
                .await?;
            match listings_outcome {
                Ok(listing_count) => {
                    ctx.metrics().set(
                        &count_metric("listings"),
                        i64::try_from(listing_count).unwrap_or(-1),
                    );
                }
                Err(error) => {
                    ctx.metrics().set(
                        "listings_error_status",
                        error.status.map_or(-1_i64, i64::from),
                    );
                    ctx.metrics().set("listings_error", format!("{error}"));
                }
            }

            ctx.mark_partial(
                "no merchant store on this avatar: the mutation routes \
                 (POST /listings, PUT /listing, PUT /associate_inventory, \
                 DELETE /listing) are not exercised — transport-level \
                 conformance ceiling; OpenSim has no DirectDelivery at all",
            );
            Ok(())
        })
    }
}
