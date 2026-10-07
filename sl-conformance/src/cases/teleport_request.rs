//! One avatar asks another to offer it a teleport, and the other does.

use sl_client_tokio::{AgentKey, Command, ImDialog, InstantMessage, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::lure::{
    NOTICE_WINDOW, offer_and_receive, record_im, record_notices, wait_for_dialog, watch_notices,
};
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};

/// Where each answer below is written down.
const SOURCE: &str = "book/src/gridspec/teleport.md (teleport-request, 2026-10-07)";

/// The id a request arrives with. It is sent with none; OpenSim gives every
/// instant message that has none the id of the two agents' conversation.
const REQUEST_ID: Measured<&str> = Measured {
    second_life: "nil",
    opensim: "conversation",
    source: SOURCE,
};

/// How the id of a delivered request reads: `nil`, `conversation` for the
/// exclusive-or of the two agent ids (the id of their one-to-one session), or
/// `other`.
fn request_id_kind(request: &InstantMessage, asked: AgentKey, requester: AgentKey) -> &'static str {
    let conversation = Uuid::from_u128(asked.uuid().as_u128() ^ requester.uuid().as_u128());
    if request.id.is_nil() {
        "nil"
    } else if request.id == conversation {
        "conversation"
    } else {
        "other"
    }
}

/// Asks for a teleport offer and receives one.
///
/// A teleport request is an `ImprovedInstantMessage` of dialog
/// `IM_TELEPORT_REQUEST` (26) with a nil id and an empty binary bucket. There
/// is no accept or decline message for it: a viewer answers yes by sending an
/// ordinary offer back (`StartLure`), and no by sending nothing.
///
/// The requester is the secondary and the one asked the primary. The case
/// records the request as it was delivered, watches the requester for
/// [`NOTICE_WINDOW`] for anything the grid says about a request nobody has
/// answered yet, and then has the primary answer with an offer, which must
/// arrive as any other offer does.
///
/// `2av`.
#[derive(Debug)]
pub struct TeleportRequest;

impl GridTest for TeleportRequest {
    fn name(&self) -> &'static str {
        "teleport-request"
    }

    fn description(&self) -> &'static str {
        "Ask another avatar for a teleport offer, record the request as delivered, and receive one"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        super::teleport_offer_accept::TeleportOfferAccept.start_location(grid)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (primary, secondary) = ctx.primary_and_secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            primary.wait_for_region(REGION_TIMEOUT).await?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;
            let primary_id = crate::lure::agent_id(primary, "asked")?;
            let secondary_id = crate::lure::agent_id(secondary, "requester")?;

            let message = format!("sl-conformance teleport-request {secondary_id}");
            secondary
                .send(Command::RequestTeleport {
                    to_agent_id: primary_id,
                    message: message.clone(),
                })
                .await?;
            let request =
                wait_for_dialog(primary, secondary_id, ImDialog::TeleportRequest, &message).await?;
            let requester_unanswered = watch_notices(secondary, NOTICE_WINDOW).await?;

            // The answer is an offer like any other.
            let answer = format!("sl-conformance teleport-request answer {primary_id}");
            let offer = offer_and_receive(primary, secondary, &answer).await?;

            let metrics = ctx.metrics();
            record_im("request_", &request, metrics);
            record_im("answer_", &offer, metrics);
            record_notices("requester_unanswered", &requester_unanswered, metrics);
            metrics.set(
                "request_id_reads",
                request_id_kind(&request, primary_id, secondary_id),
            );

            check_eq("the request's recipient", &request.to_agent_id, &primary_id)?;
            REQUEST_ID.check(
                "the id a teleport request arrives with",
                grid,
                &request_id_kind(&request, primary_id, secondary_id),
            )?;
            check(
                !request.offline,
                "a request between two logged-in avatars arrived flagged as stored",
            )?;
            check(
                requester_unanswered.is_empty(),
                "the grid said something to the requester about a request nobody had answered",
            )
        })
    }
}
