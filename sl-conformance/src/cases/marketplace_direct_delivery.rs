//! A census of the Second Life Marketplace's `DirectDelivery` capability: whether
//! a region grants it, and what the listing service behind it answers each of
//! its routes for an avatar with no merchant store.
//!
//! The SLM listing service is the protocol surface's only plain-JSON REST API
//! (Firestorm `llmarketplacefunctions.cpp`): every route is a path appended to
//! the capability's URL. The test avatars have no store, so the census asks
//! what a storeless avatar can ask and records the whole answer — status,
//! media type, any redirect and the body (ids scrubbed) — then asks the same
//! two questions through our own client's commands, so the run also says what
//! the client makes of each answer:
//!
//! 1. **The capability**: does the seed grant `DirectDelivery`?
//! 2. **The merchant probe**, `GET /merchant`, sent with no JSON headers as
//!    the reference sends it: its status *is* the answer (404 not a merchant,
//!    503 a store not migrated).
//! 3. **The listings**, `GET /listings`.
//! 4. **One listing**, `GET /listing/<id>` of an id that is nobody's.
//! 5. **A new listing**, `POST /listings` of a nil listing folder — what the
//!    service tells a non-merchant who tries to list.
//! 6. **A route the service does not have**, `GET /census-unknown-route` —
//!    whether its "not found" is the routes' answer or the capability's.
//! 7. **Our reading** of the merchant probe and of the listings
//!    ([`Command::MarketplaceMerchantStatus`] / [`Command::MarketplaceListings`]).
//!
//! The routes that change a listing in place (`PUT /listing/<id>`,
//! `PUT /associate_inventory/<id>`, `DELETE /listing/<id>`) are not asked:
//! they name a listing of the avatar's own, which a storeless avatar has
//! none of, and would name someone else's otherwise.
//!
//! A grid with no capability records `no capability` for every raw answer.
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/economy.md` § Marketplace; every run is held to it.
//!
//! `1av`, live and fake grids.

use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use sl_client_tokio::{
    Command, CreateListing, Event, InventoryFolderKey, MarketplaceInventoryInfo,
    MarketplaceOperation, MerchantStatus, Uuid,
};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::hold_to_measured;
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::REGION_TIMEOUT;

/// The capability the census measures.
const CAPABILITY: &str = "DirectDelivery";

/// How long to poll for the capability after region arrival: the seed fetch
/// races the arrival, so the map may fill in shortly afterwards.
const CAP_WAIT: Duration = Duration::from_secs(20);

/// How long each answer may take. The service is an external web service
/// behind the capability, so it gets a web budget rather than a UDP one.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// How many characters of a body the census keeps: enough for a JSON error
/// or the first words of a page, never a whole page.
const BODY_CHARS: usize = 160;

/// What a raw answer reads on a grid that grants no capability.
const NO_CAPABILITY: &str = "no capability";

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/economy.md § Marketplace (marketplace-direct-delivery, 2026-10-19)";

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// The status Second Life's service answers every route with.
const NOT_FOUND: Answer = Answer::Count(404);

/// The media type of every answer of Second Life's service.
const PLAIN_TEXT: Answer = Answer::Text("text/plain; charset=utf-8");

/// No answer of Second Life's service redirects.
const NO_REDIRECT: Answer = Answer::Text("none");

/// The body of every answer of Second Life's service.
const NOT_FOUND_BODY: Answer = Answer::Text("Not found");

/// A raw answer on a grid that grants no capability.
const NONE_GRANTED: Answer = Answer::Text(NO_CAPABILITY);

/// What each grid was measured to answer, by the metric the census records it
/// under. Second Life grants the capability and its service answers a
/// storeless avatar `404 Not found` on every route, a route it does not have
/// included; OpenSim grants none.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "capability_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    // The merchant probe.
    ("merchant_status", each(NOT_FOUND, NONE_GRANTED)),
    ("merchant_content_type", each(PLAIN_TEXT, NONE_GRANTED)),
    ("merchant_location", each(NO_REDIRECT, NONE_GRANTED)),
    ("merchant_body", each(NOT_FOUND_BODY, NONE_GRANTED)),
    // The listings.
    ("listings_status", each(NOT_FOUND, NONE_GRANTED)),
    ("listings_content_type", each(PLAIN_TEXT, NONE_GRANTED)),
    ("listings_location", each(NO_REDIRECT, NONE_GRANTED)),
    ("listings_body", each(NOT_FOUND_BODY, NONE_GRANTED)),
    // A listing that is nobody's.
    ("listing_nobodys_status", each(NOT_FOUND, NONE_GRANTED)),
    (
        "listing_nobodys_content_type",
        each(PLAIN_TEXT, NONE_GRANTED),
    ),
    ("listing_nobodys_location", each(NO_REDIRECT, NONE_GRANTED)),
    ("listing_nobodys_body", each(NOT_FOUND_BODY, NONE_GRANTED)),
    // A new listing.
    ("create_listing_status", each(NOT_FOUND, NONE_GRANTED)),
    (
        "create_listing_content_type",
        each(PLAIN_TEXT, NONE_GRANTED),
    ),
    ("create_listing_location", each(NO_REDIRECT, NONE_GRANTED)),
    ("create_listing_body", each(NOT_FOUND_BODY, NONE_GRANTED)),
    // A route the service does not have.
    ("unknown_route_status", each(NOT_FOUND, NONE_GRANTED)),
    ("unknown_route_content_type", each(PLAIN_TEXT, NONE_GRANTED)),
    ("unknown_route_location", each(NO_REDIRECT, NONE_GRANTED)),
    ("unknown_route_body", each(NOT_FOUND_BODY, NONE_GRANTED)),
    // What our client makes of it.
    (
        "client_merchant_status",
        each(
            Answer::Text("not a merchant"),
            Answer::Text(
                "connection failure: no DirectDelivery capability (not granted by this \
                 region/grid)",
            ),
        ),
    ),
    (
        "client_listings",
        each(
            Answer::Text("SLM error (HTTP 404): api error (code -, description -): Not found"),
            Answer::Text(
                "SLM error: transport failure: no DirectDelivery capability (not granted by \
                 this region/grid)",
            ),
        ),
    ),
];

/// The census of the `DirectDelivery` capability and its listing service.
#[derive(Debug)]
pub struct MarketplaceDirectDelivery;

impl GridTest for MarketplaceDirectDelivery {
    fn name(&self) -> &'static str {
        "marketplace-direct-delivery"
    }

    fn description(&self) -> &'static str {
        "Whether a region grants DirectDelivery and what the SLM listing service answers \
         each route for an avatar with no store"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx.primary(), &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// One route the census asks the service raw.
#[derive(Debug, Clone)]
struct Route {
    /// The word the route's metrics begin with.
    word: &'static str,
    /// The HTTP method.
    method: reqwest::Method,
    /// The path appended to the capability's URL.
    path: &'static str,
    /// Whether the JSON `Accept` / `Content-Type` headers go with it (the
    /// reference sends them on every route but the merchant probe).
    json_headers: bool,
}

/// Runs every leg, recording into `metrics`.
async fn census(session: &mut Session, metrics: &mut Metrics) -> Result<(), TestFailure> {
    session.wait_for_region(REGION_TIMEOUT).await?;
    let started = Instant::now();
    while session.cap(CAPABILITY).is_none() && started.elapsed() < CAP_WAIT {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let cap = session.cap(CAPABILITY);
    metrics.set("capability_granted", cap.is_some());

    // The body our client's `create_listing_request` sends, of a nil folder.
    let nil = InventoryFolderKey::from(Uuid::nil());
    let create_body = serde_json::to_string(&serde_json::json!({
        "listing": CreateListing {
            name: "SLClientMarketplaceCensus".to_owned(),
            inventory_info: MarketplaceInventoryInfo {
                listing_folder_id: nil,
                version_folder_id: nil,
                count_on_hand: 0,
            },
        }
    }))
    .map_err(|error| TestFailure::State(format!("building the listing body failed: {error}")))?;
    let routes = [
        Route {
            word: "merchant",
            method: reqwest::Method::GET,
            path: "/merchant",
            json_headers: false,
        },
        Route {
            word: "listings",
            method: reqwest::Method::GET,
            path: "/listings",
            json_headers: true,
        },
        // A listing id that is nobody's.
        Route {
            word: "listing_nobodys",
            method: reqwest::Method::GET,
            path: "/listing/1",
            json_headers: true,
        },
        Route {
            word: "create_listing",
            method: reqwest::Method::POST,
            path: "/listings",
            json_headers: true,
        },
        Route {
            word: "unknown_route",
            method: reqwest::Method::GET,
            path: "/census-unknown-route",
            json_headers: true,
        },
    ];
    let http = reqwest::Client::builder()
        .timeout(REPLY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;
    for route in routes {
        let body = (route.method == reqwest::Method::POST).then(|| create_body.clone());
        ask_raw(&http, cap.as_deref(), route, body, metrics).await;
    }

    // What our client makes of the same two answers.
    session.send(Command::MarketplaceMerchantStatus).await?;
    let status = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::MarketplaceMerchantStatus(status) => Some(status.clone()),
            _ => None,
        })
        .await?;
    metrics.set("client_merchant_status", merchant_status_word(&status));
    session.send(Command::MarketplaceListings).await?;
    let listings = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::MarketplaceListings(listings) => Some(format!("{} listings", listings.len())),
            Event::MarketplaceError {
                operation: MarketplaceOperation::GetListings,
                error,
            } => Some(without_ids(&error.to_string())),
            _ => None,
        })
        .await?;
    metrics.set("client_listings", listings);
    Ok(())
}

/// Asks `route` of the service behind `cap` and records the whole answer under
/// the route's word: `_status`, `_content_type`, `_location` and `_body`. A
/// grid with no capability, or a request that never produced an answer,
/// records that instead.
async fn ask_raw(
    http: &reqwest::Client,
    cap: Option<&str>,
    route: Route,
    body: Option<String>,
    metrics: &mut Metrics,
) {
    let word = route.word;
    let Some(cap) = cap else {
        for suffix in ["status", "content_type", "location", "body"] {
            metrics.set(&format!("{word}_{suffix}"), NO_CAPABILITY);
        }
        return;
    };
    let mut request = http.request(route.method, format!("{cap}{}", route.path));
    if route.json_headers {
        request = request
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json");
    }
    if let Some(body) = body {
        request = request.body(body);
    }
    match request.send().await {
        Ok(response) => {
            metrics.set(
                &format!("{word}_status"),
                i64::from(response.status().as_u16()),
            );
            let header = |name| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map_or_else(|| "none".to_owned(), without_ids)
            };
            metrics.set(&format!("{word}_content_type"), header(CONTENT_TYPE));
            metrics.set(&format!("{word}_location"), header(LOCATION));
            let text = response.text().await.unwrap_or_default();
            metrics.set(&format!("{word}_body"), body_words(&text));
        }
        Err(error) => {
            let failure = format!("no answer: {}", without_ids(&error.to_string()));
            for suffix in ["status", "content_type", "location", "body"] {
                metrics.set(&format!("{word}_{suffix}"), failure.as_str());
            }
        }
    }
}

/// The first [`BODY_CHARS`] characters of a body, ids scrubbed and runs of
/// whitespace made one space, or `empty`.
fn body_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return "empty".to_owned();
    }
    let joined = words.join(" ");
    without_ids(&joined.chars().take(BODY_CHARS).collect::<String>())
}

/// What our client read a merchant probe as, in a word that names no id.
fn merchant_status_word(status: &MerchantStatus) -> String {
    match status {
        MerchantStatus::Merchant => "merchant".to_owned(),
        MerchantStatus::NotMerchant => "not a merchant".to_owned(),
        MerchantStatus::NotMigratedMerchant => "merchant not migrated".to_owned(),
        MerchantStatus::ConnectionFailure { status, reason } => match status {
            Some(status) => format!("connection failure {status}: {}", without_ids(reason)),
            None => format!("connection failure: {}", without_ids(reason)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{BODY_CHARS, body_words};
    use pretty_assertions::assert_eq;

    /// A body is cut short, its whitespace evened and an empty one named.
    #[test]
    fn a_body_is_recorded_in_words() {
        assert_eq!(body_words(" \n"), "empty");
        assert_eq!(body_words("{\"a\":\n  1}"), "{\"a\": 1}");
        assert_eq!(body_words(&"x".repeat(BODY_CHARS * 2)).len(), BODY_CHARS);
    }
}
