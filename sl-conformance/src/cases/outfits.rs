//! A census of the Current Outfit Folder and the legacy wearables messages:
//! what each grid keeps as the outfit, what it answers every way a viewer
//! changes it, and what it says unasked.
//!
//! Avatar **A** alone. The census copies A's worn shape (and, where A wears a
//! clothing layer, that layer twice) so that every change it makes wears
//! something A already looks like. The legs, in order:
//!
//! 1. **The arrival**: the login's `cof_version`, the skeleton's Current
//!    Outfit Folder, and any `AgentWearablesUpdate` sent before A asked.
//! 2. **The folder**: what the COF holds — links only, which kinds, how a
//!    clothing link is described (the reference's `@<order>` token) — and,
//!    on Second Life, which keys an AIS3 link is stated with.
//! 3. **The legacy reply**: `AgentWearablesRequest`, against the folder.
//! 4. **Wearing by the folder**: a link to the shape's copy, then the old
//!    shape's link removed — the reference's road on Second Life.
//! 5. **A UDP link into the folder** (`LinkInventoryItem` sent raw).
//! 6. **Wearing by `AgentIsNowWearing`** — the legacy road.
//! 7. **A new login** with the copy worn both ways.
//! 8. **Layer order**: two layers on one slot, linked with order tokens and
//!    then with the tokens swapped; what the grid keeps, what its bake reads
//!    (Second Life's refused bake lists the worn items per slot) and what
//!    the legacy reply says.
//! 9. **An outfit folder**: a folder link (`AT_LINK_FOLDER`) into the COF.
//!
//! Every link, copy and folder the census makes is taken away again, the
//! shape's link put back and the arrival's outfit stated again, also when a
//! leg fails; a run that finds a previous run's leftovers removes them first.
//!
//! What each live grid answered is written up in
//! `book/src/gridspec/appearance.md` § Outfits; every run is held to it.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use sl_client_tokio::{
    AnyMessage, AssetType, Command, Event, FolderInfo, FolderType, InventoryFolder,
    InventoryFolderKey, InventoryItem, InventoryItemOrFolderKey, InventoryKey, InventoryType,
    NewInventoryLink, Reliability, Uuid, Wearable, WearableType,
};
use sl_wire::messages::{
    LinkInventoryItem, LinkInventoryItemAgentDataBlock, LinkInventoryItemInventoryBlockBlock,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::hold_to_measured;
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/appearance.md § Outfits (outfits, 2026-10-20)";

/// Where A logs in on OpenSim.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// The AIS3 capability.
const AIS_CAP: &str = "InventoryAPIv3";

/// The central-bake capability.
const BAKE_CAP: &str = "UpdateAvatarAppearance";

/// The bake-version capability.
const INCREMENT_CAP: &str = "IncrementCOFVersion";

/// What a leg records where the grid grants no AIS3 / bake capability.
const NO_CAPABILITY: &str = "no capability";

/// How long a leg listens on a live grid.
const WINDOW: Duration = Duration::from_secs(8);

/// How long a leg listens after the arrival.
const ARRIVAL_WINDOW: Duration = Duration::from_secs(12);

/// How long a reply may take.
const STEP: Duration = Duration::from_secs(30);

/// How long an HTTP request may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// What every item and folder the census makes is called, so a later run
/// finds an earlier one's leftovers.
const PREFIX: &str = "SLClientOutfit";

/// The shape's copy.
const SHAPE_COPY: &str = "SLClientOutfitShape";

/// The first layer copy.
const LAYER_A: &str = "SLClientOutfitLayerA";

/// The second layer copy.
const LAYER_B: &str = "SLClientOutfitLayerB";

/// The outfit folder.
const OUTFIT_FOLDER: &str = "SLClientOutfitFolder";

/// The `AT_LINK` asset type.
const AT_LINK: i8 = 24;

/// The `AT_LINK_FOLDER` asset type.
const AT_LINK_FOLDER: i8 = 25;

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

/// What each grid was measured to answer, by the metric the census records
/// it under. A grid's own appearance arriving after a change is `Any` on
/// Second Life: it follows the bake, which comes when it comes. The
/// avatar's own outfit (which slot the layer is on, what else the folder
/// holds) and the restore are recorded unheld.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("arrival_alerts", both(Answer::Text("none"))),
    ("arrival_appearance", both(Answer::Flag(true))),
    ("arrival_inventory", both(Answer::Text("none"))),
    (
        "arrival_login_cof_version",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "arrival_skeleton_cof",
        each(
            Answer::Text("listed, at the login's version"),
            Answer::Text("listed"),
        ),
    ),
    ("arrival_wearables_update", both(Answer::Text("none"))),
    (
        "cof_ais_link_keys",
        each(
            Answer::Text(
                "_links agent_id created_at desc inv_type item_id linked_id name parent_id type",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cof_ais_status",
        each(
            Answer::Text("200"),
            Answer::Text("no answer: no capability"),
        ),
    ),
    ("cof_body_parts", both(Answer::Count(4))),
    (
        "cof_clothing_order",
        both(Answer::Text("every clothing link ordered")),
    ),
    (
        "cof_listing_carries",
        each(
            Answer::Text("the folder's entries only"),
            Answer::Text("every link's target too"),
        ),
    ),
    ("cof_not_links", both(Answer::Count(0))),
    ("cof_unresolved", both(Answer::Count(0))),
    (
        "cof_version_against_login",
        each(Answer::Text("+0"), Answer::Text("no login version")),
    ),
    ("copy_shape", both(Answer::Flag(true))),
    ("now_wearing_alerts", both(Answer::Text("none"))),
    ("now_wearing_appearance", both(Answer::Flag(false))),
    ("now_wearing_cof_version_step", both(Answer::Text("+0"))),
    ("now_wearing_inventory", both(Answer::Text("none"))),
    ("now_wearing_wearables_replies", both(Answer::Count(1))),
    (
        "now_wearing_wearables_reply",
        each(
            Answer::Text(
                "4 of the reference's placeholder ids, assets d3d25329-42a7-4526-a622-7db0c33d72c3,57ca535e-1f23-49e5-a478-a443a5380564,bb1089a8-3060-4722-9692-8b8ed1ab7a29,09656fd9-a624-491d-9370-919aa721aa09, types 1,2,3,4",
            ),
            Answer::Text(
                "4 body parts, 2 clothing, every asset named; naming the shape's copy, the layer",
            ),
        ),
    ),
    ("now_wearing_wearables_update", both(Answer::Text("none"))),
    ("order_a_alerts", both(Answer::Text("none"))),
    (
        "order_a_answer",
        each(Answer::Text("ais3 201"), Answer::Text("udp item created")),
    ),
    ("order_a_appearance", both(Answer::Flag(false))),
    ("order_a_description", both(Answer::Text("as written"))),
    (
        "order_a_inventory",
        each(
            Answer::Text("none"),
            Answer::Text("a COF link to layer a (udp)"),
        ),
    ),
    (
        "order_a_link_keys",
        each(
            Answer::Text(
                "_broken _embedded _links agent_id created_at desc inv_type item_id linked_id name parent_id type",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("order_a_link_type", both(Answer::Count(24))),
    ("order_a_listed", both(Answer::Flag(true))),
    ("order_a_version_step", both(Answer::Text("+1"))),
    (
        "order_a_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    ("order_a_wearables_update", both(Answer::Text("none"))),
    ("order_b_alerts", both(Answer::Text("none"))),
    (
        "order_b_answer",
        each(Answer::Text("ais3 201"), Answer::Text("udp item created")),
    ),
    ("order_b_appearance", both(Answer::Flag(false))),
    ("order_b_description", both(Answer::Text("as written"))),
    (
        "order_b_inventory",
        each(
            Answer::Text("none"),
            Answer::Text("a COF link to layer b (udp)"),
        ),
    ),
    (
        "order_b_link_keys",
        each(
            Answer::Text(
                "_broken _embedded _links agent_id created_at desc inv_type item_id linked_id name parent_id type",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("order_b_link_type", both(Answer::Count(24))),
    ("order_b_listed", both(Answer::Flag(true))),
    ("order_b_version_step", both(Answer::Text("+1"))),
    (
        "order_b_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    ("order_b_wearables_update", both(Answer::Text("none"))),
    (
        "order_bake",
        each(
            Answer::Text("the layer, layer a, layer b; every one described by its link's token"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "order_bake_baked",
        each(Answer::Text("accepted"), Answer::Text("no capability")),
    ),
    (
        "order_bake_increment",
        each(
            Answer::Text("the folder's version +1"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "order_reply",
        each(
            Answer::Text("nothing on the slot"),
            Answer::Text("the layer, layer a, layer b"),
        ),
    ),
    (
        "order_swap_a_answer",
        each(Answer::Text("ais3 200"), Answer::Text("udp sent")),
    ),
    ("order_swap_a_description", both(Answer::Text("as written"))),
    ("order_swap_a_version_step", both(Answer::Text("+1"))),
    (
        "order_swap_b_answer",
        each(Answer::Text("ais3 200"), Answer::Text("udp sent")),
    ),
    ("order_swap_b_description", both(Answer::Text("as written"))),
    ("order_swap_b_version_step", both(Answer::Text("+1"))),
    (
        "order_swapped_bake",
        each(
            Answer::Text("the layer, layer b, layer a; every one described by its link's token"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "order_swapped_bake_baked",
        each(Answer::Text("accepted"), Answer::Text("no capability")),
    ),
    (
        "order_swapped_bake_increment",
        each(
            Answer::Text("the folder's version +1"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "order_swapped_reply",
        each(
            Answer::Text("nothing on the slot"),
            Answer::Text("the layer, layer b, layer a"),
        ),
    ),
    ("order_swapped_wearables_replies", both(Answer::Count(1))),
    (
        "order_swapped_wearables_reply",
        each(
            Answer::Text(
                "4 of the reference's placeholder ids, assets d3d25329-42a7-4526-a622-7db0c33d72c3,57ca535e-1f23-49e5-a478-a443a5380564,bb1089a8-3060-4722-9692-8b8ed1ab7a29,09656fd9-a624-491d-9370-919aa721aa09, types 1,2,3,4",
            ),
            Answer::Text(
                "4 body parts, 4 clothing, every asset named; naming the shape's copy, the layer, layer b, layer a",
            ),
        ),
    ),
    ("order_swapped_wearing_alerts", both(Answer::Text("none"))),
    (
        "order_swapped_wearing_appearance",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "order_swapped_wearing_cof_version_step",
        each(Answer::Text("+1"), Answer::Text("+0")),
    ),
    (
        "order_swapped_wearing_inventory",
        both(Answer::Text("none")),
    ),
    (
        "order_swapped_wearing_wearables_update",
        both(Answer::Text("none")),
    ),
    ("order_unlink_a_alerts", both(Answer::Text("none"))),
    (
        "order_unlink_a_answer",
        each(Answer::Text("ais3 200"), Answer::Text("udp sent")),
    ),
    ("order_unlink_a_appearance", both(Answer::Flag(false))),
    ("order_unlink_a_gone", both(Answer::Flag(true))),
    ("order_unlink_a_inventory", both(Answer::Text("none"))),
    ("order_unlink_a_version_step", both(Answer::Text("+1"))),
    (
        "order_unlink_a_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    (
        "order_unlink_a_wearables_update",
        both(Answer::Text("none")),
    ),
    ("order_unlink_b_alerts", both(Answer::Text("none"))),
    (
        "order_unlink_b_answer",
        each(Answer::Text("ais3 200"), Answer::Text("udp sent")),
    ),
    ("order_unlink_b_appearance", both(Answer::Flag(false))),
    ("order_unlink_b_gone", both(Answer::Flag(true))),
    ("order_unlink_b_inventory", both(Answer::Text("none"))),
    ("order_unlink_b_version_step", both(Answer::Text("+1"))),
    (
        "order_unlink_b_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    (
        "order_unlink_b_wearables_update",
        both(Answer::Text("none")),
    ),
    ("order_wearables_replies", both(Answer::Count(1))),
    (
        "order_wearables_reply",
        each(
            Answer::Text(
                "4 of the reference's placeholder ids, assets d3d25329-42a7-4526-a622-7db0c33d72c3,57ca535e-1f23-49e5-a478-a443a5380564,bb1089a8-3060-4722-9692-8b8ed1ab7a29,09656fd9-a624-491d-9370-919aa721aa09, types 1,2,3,4",
            ),
            Answer::Text(
                "4 body parts, 4 clothing, every asset named; naming the shape's copy, the layer, layer a, layer b",
            ),
        ),
    ),
    ("order_wearing_alerts", both(Answer::Text("none"))),
    (
        "order_wearing_appearance",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "order_wearing_cof_version_step",
        each(Answer::Text("+1"), Answer::Text("+0")),
    ),
    ("order_wearing_inventory", both(Answer::Text("none"))),
    ("order_wearing_wearables_update", both(Answer::Text("none"))),
    (
        "outfit_folder_answer",
        each(Answer::Text("ais3 201"), Answer::Text("udp sent")),
    ),
    ("outfit_folder_made", both(Answer::Flag(true))),
    ("outfit_link_alerts", both(Answer::Text("none"))),
    (
        "outfit_link_answer",
        each(Answer::Text("ais3 201"), Answer::Text("udp item created")),
    ),
    ("outfit_link_appearance", both(Answer::Flag(false))),
    ("outfit_link_description", both(Answer::Text("as written"))),
    (
        "outfit_link_inventory",
        each(
            Answer::Text("none"),
            Answer::Text("a COF link to the outfit folder (udp)"),
        ),
    ),
    (
        "outfit_link_link_keys",
        each(
            Answer::Text(
                "_broken _embedded _links agent_id created_at desc inv_type item_id linked_id name parent_id type",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("outfit_link_link_type", both(Answer::Count(25))),
    ("outfit_link_listed", both(Answer::Flag(true))),
    ("outfit_link_version_step", both(Answer::Text("+1"))),
    (
        "outfit_link_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    ("outfit_link_wearables_update", both(Answer::Text("none"))),
    ("outfit_unlink_alerts", both(Answer::Text("none"))),
    (
        "outfit_unlink_answer",
        each(Answer::Text("ais3 200"), Answer::Text("udp sent")),
    ),
    ("outfit_unlink_appearance", both(Answer::Flag(false))),
    ("outfit_unlink_gone", both(Answer::Flag(true))),
    ("outfit_unlink_inventory", both(Answer::Text("none"))),
    ("outfit_unlink_version_step", both(Answer::Text("+1"))),
    (
        "outfit_unlink_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    ("outfit_unlink_wearables_update", both(Answer::Text("none"))),
    ("relogin_alerts", both(Answer::Text("none"))),
    ("relogin_appearance", both(Answer::Flag(true))),
    ("relogin_cof_shape", both(Answer::Text("the copy"))),
    ("relogin_cof_version_step", both(Answer::Text("+0"))),
    ("relogin_inventory", both(Answer::Text("none"))),
    (
        "relogin_login_cof_version",
        each(Answer::Text("+0"), Answer::Text("none")),
    ),
    ("relogin_wearables_replies", both(Answer::Count(1))),
    (
        "relogin_wearables_reply",
        each(
            Answer::Text(
                "4 of the reference's placeholder ids, assets d3d25329-42a7-4526-a622-7db0c33d72c3,57ca535e-1f23-49e5-a478-a443a5380564,bb1089a8-3060-4722-9692-8b8ed1ab7a29,09656fd9-a624-491d-9370-919aa721aa09, types 1,2,3,4",
            ),
            Answer::Text(
                "4 body parts, 2 clothing, every asset named; naming the shape's copy, the layer",
            ),
        ),
    ),
    ("relogin_wearables_update", both(Answer::Text("none"))),
    (
        "udp_link_alerts",
        each(
            Answer::Text("AlertMessage: Cannot create requested inventory."),
            Answer::Text("none"),
        ),
    ),
    (
        "udp_link_appearance",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "udp_link_inventory",
        each(
            Answer::Text("none"),
            Answer::Text("a COF link to the shape (udp)"),
        ),
    ),
    (
        "udp_link_listed",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    ("udp_link_version_step", both(Answer::Text("+1"))),
    ("udp_link_wearables_update", both(Answer::Text("none"))),
    (
        "udp_unlink_alerts",
        each(Answer::Text("no link made"), Answer::Text("none")),
    ),
    (
        "udp_unlink_answer",
        each(Answer::Text("no link made"), Answer::Text("udp sent")),
    ),
    (
        "udp_unlink_appearance",
        each(Answer::Text("no link made"), Answer::Flag(false)),
    ),
    (
        "udp_unlink_gone",
        each(Answer::Text("no link made"), Answer::Flag(true)),
    ),
    (
        "udp_unlink_inventory",
        each(Answer::Text("no link made"), Answer::Text("none")),
    ),
    (
        "udp_unlink_version_step",
        each(Answer::Text("no link made"), Answer::Text("+1")),
    ),
    (
        "udp_unlink_versions",
        each(Answer::Text("no link made"), Answer::Text("no capability")),
    ),
    (
        "udp_unlink_wearables_update",
        each(Answer::Text("no link made"), Answer::Text("none")),
    ),
    (
        "wear_bake",
        each(
            Answer::Text("accepted at the folder's version +1"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "wear_bake_appearance",
        each(Answer::Any, Answer::Text("no capability")),
    ),
    ("wear_link_alerts", both(Answer::Text("none"))),
    (
        "wear_link_answer",
        each(Answer::Text("ais3 201"), Answer::Text("udp item created")),
    ),
    ("wear_link_appearance", both(Answer::Flag(false))),
    ("wear_link_description", both(Answer::Text("as written"))),
    (
        "wear_link_inventory",
        each(
            Answer::Text("none"),
            Answer::Text("a COF link to the shape's copy (udp)"),
        ),
    ),
    (
        "wear_link_link_keys",
        each(
            Answer::Text(
                "_broken _embedded _links agent_id created_at desc inv_type item_id linked_id name parent_id type",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("wear_link_link_type", both(Answer::Count(24))),
    ("wear_link_listed", both(Answer::Flag(true))),
    ("wear_link_version_step", both(Answer::Text("+1"))),
    (
        "wear_link_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    ("wear_link_wearables_update", both(Answer::Text("none"))),
    ("wear_unlink_alerts", both(Answer::Text("none"))),
    (
        "wear_unlink_answer",
        each(Answer::Text("ais3 200"), Answer::Text("udp sent")),
    ),
    ("wear_unlink_appearance", both(Answer::Flag(false))),
    ("wear_unlink_gone", both(Answer::Flag(true))),
    ("wear_unlink_inventory", both(Answer::Text("none"))),
    ("wear_unlink_version_step", both(Answer::Text("+1"))),
    (
        "wear_unlink_versions",
        each(Answer::Text("cof +1"), Answer::Text("no capability")),
    ),
    ("wear_unlink_wearables_update", both(Answer::Text("none"))),
    ("wear_wearables_replies", both(Answer::Count(1))),
    (
        "wear_wearables_reply",
        each(
            Answer::Text(
                "4 of the reference's placeholder ids, assets d3d25329-42a7-4526-a622-7db0c33d72c3,57ca535e-1f23-49e5-a478-a443a5380564,bb1089a8-3060-4722-9692-8b8ed1ab7a29,09656fd9-a624-491d-9370-919aa721aa09, types 1,2,3,4",
            ),
            Answer::Text(
                "4 body parts, 2 clothing, every asset named; naming the shape, the layer",
            ),
        ),
    ),
    (
        "wearables_against_cof",
        each(
            Answer::Text("other items"),
            Answer::Text("the folder's wearables"),
        ),
    ),
    ("wearables_replies", both(Answer::Count(1))),
    (
        "wearables_reply",
        each(
            Answer::Text(
                "4 of the reference's placeholder ids, assets d3d25329-42a7-4526-a622-7db0c33d72c3,57ca535e-1f23-49e5-a478-a443a5380564,bb1089a8-3060-4722-9692-8b8ed1ab7a29,09656fd9-a624-491d-9370-919aa721aa09, types 1,2,3,4",
            ),
            Answer::Text(
                "4 body parts, 2 clothing, every asset named; naming the shape, the layer",
            ),
        ),
    ),
];

/// The outfit census.
#[derive(Debug)]
pub struct Outfits;

impl GridTest for Outfits {
    fn name(&self) -> &'static str {
        "outfits"
    }

    fn description(&self) -> &'static str {
        "Record each grid's Current Outfit Folder and legacy wearables: what is kept, what \
         every way of wearing answers, what is said unasked, layer order and outfit links"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(900)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// What A was told during a window.
#[derive(Debug, Default)]
struct Told {
    /// Every `AgentWearablesUpdate`.
    wearables: Vec<Vec<Wearable>>,
    /// How many of A's own `AvatarAppearance`s.
    appearances: usize,
    /// The alerts.
    alerts: Vec<String>,
    /// Items announced over UDP (`UpdateCreateInventoryItem`).
    created: Vec<InventoryItem>,
    /// Items announced in a bulk update.
    bulk: Vec<InventoryItem>,
    /// Items said to be removed.
    removed: Vec<InventoryKey>,
    /// The folders of a skeleton, where one came.
    skeleton: Option<Vec<InventoryFolder>>,
}

impl Told {
    /// Folds one event in; `actor` is A's id.
    fn note(&mut self, actor: Uuid, event: &Event) {
        match event {
            Event::AgentWearables { wearables, .. } => self.wearables.push(wearables.clone()),
            Event::AvatarAppearance(appearance) if appearance.avatar_id.uuid() == actor => {
                self.appearances = self.appearances.saturating_add(1);
            }
            Event::AlertMessage { message, .. } => {
                self.alerts
                    .push(format!("AlertMessage: {}", without_ids(message)));
            }
            Event::AgentAlertMessage { message, .. } => {
                self.alerts
                    .push(format!("AgentAlertMessage: {}", without_ids(message)));
            }
            Event::InventoryItemCreated { item, .. } => self.created.push(item.clone()),
            Event::InventoryBulkUpdate { items, .. } => self.bulk.extend(items.iter().cloned()),
            Event::InventoryItemsRemoved { items }
            | Event::InventoryObjectsRemoved { items, .. } => {
                self.removed.extend(items.iter().copied());
            }
            Event::InventorySkeleton(folders) => self.skeleton = Some(folders.clone()),
            _ => {}
        }
    }
}

/// Listens on `session` for `window`, folding everything into `told`.
async fn listen(
    session: &mut Session,
    told: &mut Told,
    actor: Uuid,
    window: Duration,
) -> Result<(), TestFailure> {
    match session
        .wait_for(window, |event| {
            told.note(actor, event);
            None::<()>
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Listens on `session` until `done` holds, folding everything into `told`,
/// then a moment more; for `limit` at most.
async fn listen_until(
    session: &mut Session,
    told: &mut Told,
    actor: Uuid,
    limit: Duration,
    done: impl Fn(&Told) -> bool + Send + Sync,
) -> Result<(), TestFailure> {
    let outcome = session
        .wait_for(limit, |event| {
            told.note(actor, event);
            done(told).then_some(())
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => {}
        Err(other) => return Err(other),
    }
    listen(session, told, actor, Duration::from_secs(1)).await
}

/// One entry of the Current Outfit Folder.
#[derive(Debug, Clone)]
struct Entry {
    /// The entry's own id.
    item_id: InventoryKey,
    /// What it links to (the link's `asset_id`).
    target: Uuid,
    /// Its asset type: 24 an item link, 25 a folder link.
    link_type: i8,
    /// Its description.
    description: String,
}

/// The Current Outfit Folder as read.
#[derive(Debug, Clone, Default)]
struct CofRead {
    /// The folder's version.
    version: i32,
    /// Its entries.
    entries: Vec<Entry>,
    /// How many items of other folders the listing carried besides.
    carried: usize,
}

impl CofRead {
    /// The entry linking to `target`.
    fn link_to(&self, target: Uuid) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.target == target)
    }
}

/// What an HTTP request answered: the status and the LLSD body.
#[derive(Debug)]
struct Raw {
    /// The status, or why there was none.
    status: Result<u16, String>,
    /// The body, where it was LLSD.
    body: Option<Llsd>,
}

impl Raw {
    /// The status as a metric records it.
    fn status_word(&self) -> String {
        match &self.status {
            Ok(status) => status.to_string(),
            Err(why) => format!("no answer: {why}"),
        }
    }
}

/// The items and folders a census made, and what it took away, so that the
/// restore puts everything back.
#[derive(Debug, Default)]
struct Residue {
    /// Links the census made in the COF.
    links: Vec<InventoryKey>,
    /// Items it copied.
    copies: Vec<InventoryKey>,
    /// Folders it made.
    folders: Vec<InventoryFolderKey>,
    /// Whether the shape's own link was removed.
    shape_unlinked: bool,
    /// Whether an `AgentIsNowWearing` named something else than the arrival.
    wearing_changed: bool,
}

/// Everything the legs share.
struct Census<'a> {
    /// A's session.
    a: &'a mut Session,
    /// A's id.
    actor: Uuid,
    /// A's session id, for a raw message.
    session_id: Uuid,
    /// How long a leg listens.
    window: Duration,
    /// The HTTP client.
    http: reqwest::Client,
    /// The Current Outfit Folder.
    cof: InventoryFolderKey,
    /// The inventory root.
    root: InventoryFolderKey,
    /// The Trash.
    trash: InventoryFolderKey,
    /// Every agent item read, by id.
    items: HashMap<Uuid, InventoryItem>,
    /// The role each census-relevant item plays, for a metric.
    roles: Vec<(Uuid, &'static str)>,
    /// What to put back.
    residue: Residue,
    /// The COF as last read.
    last: CofRead,
}

impl Census<'_> {
    /// The AIS3 base, where the region grants it.
    fn ais(&self) -> Option<String> {
        self.a.cap(AIS_CAP)
    }

    /// Reads the COF's contents through the client's own road (AIS3 on
    /// Second Life, the descendents capability on OpenSim).
    async fn read_cof(&mut self) -> Result<CofRead, TestFailure> {
        let cof = self.cof;
        self.a.send(Command::RequestFolderContents(cof)).await?;
        let read = self
            .a
            .wait_for(STEP, |event| match event {
                Event::InventoryDescendents {
                    folder_id,
                    version,
                    items,
                    ..
                } if *folder_id == cof => Some(CofRead {
                    version: *version,
                    carried: items.iter().filter(|item| item.folder_id != cof).count(),
                    entries: items
                        .iter()
                        .filter(|item| item.folder_id == cof)
                        .map(|item| Entry {
                            item_id: item.item_id,
                            target: item.asset_id,
                            link_type: item.item_type,
                            description: item.description.clone(),
                        })
                        .collect(),
                }),
                _ => None,
            })
            .await?;
        self.last = read.clone();
        Ok(read)
    }

    /// Sends one AIS3 request raw.
    async fn raw(&self, method: reqwest::Method, suffix: &str, body: Option<String>) -> Raw {
        let Some(base) = self.ais() else {
            return Raw {
                status: Err(NO_CAPABILITY.to_owned()),
                body: None,
            };
        };
        let mut request = self
            .http
            .request(method, format!("{base}{suffix}"))
            .header(ACCEPT, "application/llsd+xml");
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/llsd+xml")
                .body(body);
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                let body = Some(text.as_str())
                    .filter(|text| text.contains("<llsd"))
                    .and_then(|text| parse_llsd_xml(text).ok());
                Raw {
                    status: Ok(status),
                    body,
                }
            }
            Err(error) => Raw {
                status: Err(without_ids(&error.to_string())),
                body: None,
            },
        }
    }

    /// The role of `id`, or `other`.
    fn role(&self, id: Uuid) -> &'static str {
        self.roles
            .iter()
            .find(|(known, _role)| *known == id)
            .map_or("other", |(_known, role)| role)
    }

    /// The wearable slot of the item `id`, as its flags state it.
    fn slot_of(&self, id: Uuid) -> Option<WearableType> {
        let item = self.items.get(&id)?;
        let asset = AssetType::from_code(i32::from(item.item_type));
        matches!(asset, AssetType::Bodypart | AssetType::Clothing)
            .then(|| WearableType::from_code(u8::try_from(item.flags & 0xFF).unwrap_or(0)))
    }

    /// A wearables list described: how many, how many body parts and
    /// clothing, and the census roles it names, in order.
    fn describe_wearables(&self, wearables: &[Wearable]) -> String {
        if wearables.is_empty() {
            return "empty".to_owned();
        }
        if Wearable::is_placeholder_outfit(wearables) {
            return format!(
                "{} of the reference's placeholder ids, {}, types {}",
                wearables.len(),
                if wearables
                    .iter()
                    .all(|worn| worn.asset_id.is_none_or(|id| id.is_nil()))
                {
                    "no asset named".to_owned()
                } else {
                    format!(
                        "assets {}",
                        wearables
                            .iter()
                            .map(|worn| worn.asset_id.unwrap_or_default().to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                },
                wearables
                    .iter()
                    .map(|worn| worn.wearable_type.to_code().to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        let body_parts = wearables
            .iter()
            .filter(|worn| worn.wearable_type.is_body_part())
            .count();
        let named: Vec<&str> = wearables
            .iter()
            .map(|worn| self.role(worn.item_id.uuid()))
            .filter(|role| *role != "other")
            .collect();
        let assets = if wearables
            .iter()
            .all(|worn| worn.asset_id.is_some_and(|id| !id.is_nil()))
        {
            "every asset named"
        } else if wearables
            .iter()
            .all(|worn| worn.asset_id.is_none_or(|id| id.is_nil()))
        {
            "no asset named"
        } else {
            "some assets named"
        };
        format!(
            "{} body parts, {} clothing, {assets}; naming {}",
            body_parts,
            wearables.len().saturating_sub(body_parts),
            if named.is_empty() {
                "none of the census's".to_owned()
            } else {
                named.join(", ")
            }
        )
    }

    /// Records what A was told under `key`.
    fn record_told(&self, metrics: &mut Metrics, key: &str, told: &Told) {
        metrics.set(
            &format!("{key}_wearables_update"),
            told.wearables.last().map_or_else(
                || "none".to_owned(),
                |last| format!("sent: {}", self.describe_wearables(last)),
            ),
        );
        metrics.set(&format!("{key}_appearance"), told.appearances > 0);
        metrics.set(
            &format!("{key}_alerts"),
            if told.alerts.is_empty() {
                "none".to_owned()
            } else {
                told.alerts.join(" | ")
            },
        );
        let mut inventory = Vec::new();
        for (word, items) in [("udp", &told.created), ("bulk", &told.bulk)] {
            for item in items {
                inventory.push(format!("{} ({word})", self.item_word(item)));
            }
        }
        for removed in &told.removed {
            inventory.push(format!("{} removed", self.role(removed.uuid())));
        }
        metrics.set(
            &format!("{key}_inventory"),
            if inventory.is_empty() {
                "none".to_owned()
            } else {
                inventory.join(", ")
            },
        );
    }

    /// An announced item described: a COF link to a role, or the role.
    fn item_word(&self, item: &InventoryItem) -> String {
        if item.folder_id == self.cof
            && (item.item_type == AT_LINK || item.item_type == AT_LINK_FOLDER)
        {
            format!("a COF link to {}", self.role(item.asset_id))
        } else {
            self.role(item.item_id.uuid()).to_owned()
        }
    }

    /// Asks for the legacy wearables and records the reply under `key`.
    async fn wearables(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
    ) -> Result<Option<Vec<Wearable>>, TestFailure> {
        let mut told = Told::default();
        self.a.send(Command::RequestWearables).await?;
        let actor = self.actor;
        listen_until(self.a, &mut told, actor, STEP, |told| {
            !told.wearables.is_empty()
        })
        .await?;
        let reply = told.wearables.first().cloned();
        metrics.set(
            &format!("{key}_reply"),
            reply
                .as_ref()
                .map_or_else(|| "none".to_owned(), |reply| self.describe_wearables(reply)),
        );
        metrics.set(
            &format!("{key}_replies"),
            i64::try_from(told.wearables.len()).unwrap_or(i64::MAX),
        );
        Ok(reply)
    }

    /// Links `target` into the COF with `description` — over AIS3 where the
    /// region grants it, over UDP otherwise — and records the answer under
    /// `key`. Returns the link's id, where the folder lists one.
    async fn link(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        target: InventoryItemOrFolderKey,
        inv_type: InventoryType,
        description: &str,
    ) -> Result<Option<InventoryKey>, TestFailure> {
        let before = self.last.version;
        let (link_type, name) = match target {
            InventoryItemOrFolderKey::Item(item) => (
                AT_LINK,
                self.items
                    .get(&item.uuid())
                    .map_or_else(String::new, |item| item.name.clone()),
            ),
            InventoryItemOrFolderKey::Folder(_) => (AT_LINK_FOLDER, OUTFIT_FOLDER.to_owned()),
        };
        let mut told = Told::default();
        let actor = self.actor;
        if self.ais().is_some() {
            let body = sl_wire::build_ais_create_link_body(
                target.uuid(),
                i32::from(link_type),
                inv_type.to_code(),
                &name,
                description,
            );
            let raw = self
                .raw(
                    reqwest::Method::POST,
                    &sl_wire::ais_create_category_url(self.cof, Uuid::new_v4()),
                    Some(body),
                )
                .await;
            metrics.set(
                &format!("{key}_answer"),
                format!("ais3 {}", raw.status_word()),
            );
            metrics.set(
                &format!("{key}_versions"),
                self.version_steps(raw.body.as_ref(), before),
            );
            metrics.set(
                &format!("{key}_link_keys"),
                raw.body
                    .as_ref()
                    .and_then(first_link)
                    .map_or_else(|| "no link".to_owned(), key_list),
            );
            listen(self.a, &mut told, actor, self.window).await?;
        } else {
            self.a
                .send(Command::LinkInventoryItem(NewInventoryLink {
                    folder_id: self.cof,
                    linked_id: target,
                    link_type: AssetType::Other(i32::from(link_type)),
                    inv_type,
                    name,
                    description: description.to_owned(),
                }))
                .await?;
            let wanted = target.uuid();
            listen_until(self.a, &mut told, actor, self.window, |told| {
                told.created.iter().any(|item| item.asset_id == wanted)
            })
            .await?;
            metrics.set(
                &format!("{key}_answer"),
                if told.created.iter().any(|item| item.asset_id == wanted) {
                    "udp item created"
                } else {
                    "udp nothing"
                },
            );
            metrics.set(&format!("{key}_versions"), NO_CAPABILITY);
            metrics.set(&format!("{key}_link_keys"), NO_CAPABILITY);
        }
        self.record_told(metrics, key, &told);
        let read = self.read_cof().await?;
        metrics.set(&format!("{key}_version_step"), step(before, read.version));
        let listed = read.link_to(target.uuid()).cloned();
        metrics.set(&format!("{key}_listed"), listed.is_some());
        metrics.set(
            &format!("{key}_description"),
            listed.as_ref().map_or_else(
                || "not listed".to_owned(),
                |entry| describe_kept(&entry.description, description),
            ),
        );
        metrics.set(
            &format!("{key}_link_type"),
            listed
                .as_ref()
                .map_or(-1, |entry| i64::from(entry.link_type)),
        );
        if let Some(entry) = listed.as_ref() {
            self.residue.links.push(entry.item_id);
        }
        Ok(listed.map(|entry| entry.item_id))
    }

    /// The step `body`'s `_updated_category_versions` states for the COF.
    fn version_steps(&self, body: Option<&Llsd>, before: i32) -> String {
        let Some(versions) = body
            .and_then(|body| body.get("_updated_category_versions"))
            .and_then(Llsd::as_map)
        else {
            return "none stated".to_owned();
        };
        let cof = self.cof.to_string();
        let mut words: Vec<String> = versions
            .iter()
            .map(|(folder, version)| {
                if folder.trim() == cof {
                    format!("cof {}", step(before, version.as_i32().unwrap_or(0)))
                } else {
                    "another folder".to_owned()
                }
            })
            .collect();
        words.sort_unstable();
        words.join(", ")
    }

    /// Removes the COF link `link` — over AIS3 where granted, over UDP
    /// otherwise — and records the answer under `key`.
    async fn unlink(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        link: InventoryKey,
    ) -> Result<(), TestFailure> {
        let before = self.last.version;
        let mut told = Told::default();
        let actor = self.actor;
        if self.ais().is_some() {
            let raw = self
                .raw(reqwest::Method::DELETE, &sl_wire::ais_item_url(link), None)
                .await;
            metrics.set(
                &format!("{key}_answer"),
                format!("ais3 {}", raw.status_word()),
            );
            metrics.set(
                &format!("{key}_versions"),
                self.version_steps(raw.body.as_ref(), before),
            );
        } else {
            self.a
                .send(Command::RemoveInventoryItems(vec![link]))
                .await?;
            metrics.set(&format!("{key}_answer"), "udp sent");
            metrics.set(&format!("{key}_versions"), NO_CAPABILITY);
        }
        listen(self.a, &mut told, actor, self.window).await?;
        self.record_told(metrics, key, &told);
        let read = self.read_cof().await?;
        metrics.set(&format!("{key}_version_step"), step(before, read.version));
        let gone = !read.entries.iter().any(|entry| entry.item_id == link);
        metrics.set(&format!("{key}_gone"), gone);
        if gone {
            self.residue.links.retain(|known| *known != link);
        }
        Ok(())
    }

    /// Rewrites the description of the COF link `link` and records the
    /// answer under `key`.
    async fn describe(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        link: InventoryKey,
        description: &str,
    ) -> Result<(), TestFailure> {
        let before = self.last.version;
        let name = self
            .items
            .get(
                &self
                    .last
                    .entries
                    .iter()
                    .find(|entry| entry.item_id == link)
                    .map_or_else(Uuid::nil, |entry| entry.target),
            )
            .map_or_else(String::new, |item| item.name.clone());
        if self.ais().is_some() {
            let raw = self
                .raw(
                    reqwest::Method::PATCH,
                    &sl_wire::ais_item_url(link),
                    Some(sl_wire::build_ais_update_item_body(&name, description)),
                )
                .await;
            metrics.set(
                &format!("{key}_answer"),
                format!("ais3 {}", raw.status_word()),
            );
        } else {
            // A link's record is read back from the folder: rewrite its
            // description on the copy the folder listed.
            let item = self.link_record(link).await?;
            if let Some(mut item) = item {
                item.description = description.to_owned();
                self.a
                    .send(Command::UpdateInventoryItem {
                        item: Box::new(item),
                        transaction_id: sl_client_tokio::TransactionId::from(Uuid::nil()),
                    })
                    .await?;
                metrics.set(&format!("{key}_answer"), "udp sent");
            } else {
                metrics.set(&format!("{key}_answer"), "link not found");
            }
        }
        let mut told = Told::default();
        let actor = self.actor;
        listen(self.a, &mut told, actor, self.window).await?;
        let read = self.read_cof().await?;
        metrics.set(&format!("{key}_version_step"), step(before, read.version));
        metrics.set(
            &format!("{key}_description"),
            read.entries
                .iter()
                .find(|entry| entry.item_id == link)
                .map_or_else(
                    || "not listed".to_owned(),
                    |entry| describe_kept(&entry.description, description),
                ),
        );
        Ok(())
    }

    /// The full record of the COF link `link`, read from the folder.
    async fn link_record(
        &mut self,
        link: InventoryKey,
    ) -> Result<Option<InventoryItem>, TestFailure> {
        let cof = self.cof;
        self.a.send(Command::RequestFolderContents(cof)).await?;
        self.a
            .wait_for(STEP, |event| match event {
                Event::InventoryDescendents {
                    folder_id, items, ..
                } if *folder_id == cof => {
                    Some(items.iter().find(|item| item.item_id == link).cloned())
                }
                _ => None,
            })
            .await
    }

    /// Sends `set` as the agent's outfit and records what A is told under
    /// `key`.
    async fn now_wearing(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        set: Vec<Wearable>,
    ) -> Result<(), TestFailure> {
        let before = self.last.version;
        let mut told = Told::default();
        self.a.send(Command::SetWearing(set)).await?;
        let actor = self.actor;
        listen(self.a, &mut told, actor, self.window).await?;
        self.record_told(metrics, key, &told);
        let read = self.read_cof().await?;
        metrics.set(
            &format!("{key}_cof_version_step"),
            step(before, read.version),
        );
        Ok(())
    }

    /// Bumps the bake's outfit version (`IncrementCOFVersion`), as the
    /// reference does before every bake, returning the version it answers.
    async fn increment(&mut self) -> Result<Option<i32>, TestFailure> {
        if self.a.cap(INCREMENT_CAP).is_none() {
            return Ok(None);
        }
        self.a.send(Command::IncrementCofVersion).await?;
        let answer = self
            .a
            .wait_for(STEP, |event| match event {
                Event::CofVersionIncremented { version } => Some(*version),
                _ => None,
            })
            .await;
        match answer {
            Ok(version) => Ok(version),
            Err(TestFailure::Timeout(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// POSTs a bake of `cof_version`, asking again — up to four more times,
    /// after 2, 4, 8 and 16 seconds, as the reference backs off — while Second
    /// Life's bake service answers that it could not fetch an asset, which it
    /// does now and then for no reason the request has (`"Asset download
    /// error: Connection Error"`).
    async fn bake(&self, cof_version: i32) -> Option<Llsd> {
        let mut answer = self.bake_once(cof_version).await;
        for wait in [2, 4, 8, 16] {
            let transient = answer
                .as_ref()
                .and_then(|answer| answer.get("error"))
                .and_then(Llsd::as_str)
                .is_some_and(|error| error.starts_with("Asset download error"));
            if !transient {
                break;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            answer = self.bake_once(cof_version).await;
        }
        answer
    }

    /// POSTs a bake of `cof_version` raw, returning the answer.
    async fn bake_once(&self, cof_version: i32) -> Option<Llsd> {
        let cap = self.a.cap(BAKE_CAP)?;
        let response = self
            .http
            .post(&cap)
            .header(CONTENT_TYPE, "application/llsd+xml")
            .body(sl_wire::build_update_avatar_appearance_request(cof_version))
            .send()
            .await
            .ok()?;
        let text = response.text().await.ok()?;
        Some(text.as_str())
            .filter(|text| text.contains("<llsd"))
            .and_then(|text| parse_llsd_xml(text).ok())
    }

    /// The worn items of `slot` a refused bake lists, as census roles in the
    /// order listed — or why there are none.
    async fn bake_order(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        slot: WearableType,
    ) -> Result<(), TestFailure> {
        if self.a.cap(BAKE_CAP).is_none() {
            for suffix in ["", "_baked", "_cof_raw", "_increment"] {
                metrics.set(&format!("{key}{suffix}"), NO_CAPABILITY);
            }
            return Ok(());
        }
        let before = self.last.version;
        let version = self.increment().await?;
        metrics.set(
            &format!("{key}_increment"),
            version.map_or_else(
                || "no version".to_owned(),
                |version| format!("the folder's version {}", step(before, version)),
            ),
        );
        // A refusal dumps the outfit of the last bake the grid accepted, so
        // the folder as it stands is baked first.
        let accepted = match version {
            Some(version) => describe_bake(self.bake(version).await.as_ref(), version),
            None => "no version to bake".to_owned(),
        };
        metrics.set(&format!("{key}_baked"), accepted);
        let answer = self.bake(0).await;
        metrics.set(key, self.listed_order(answer.as_ref(), slot));
        metrics.set(
            &format!("{key}_cof_raw"),
            answer
                .as_ref()
                .and_then(|answer| answer.get("cof_raw"))
                .and_then(Llsd::as_array)
                .map_or_else(
                    || "none".to_owned(),
                    |entries| {
                        let mut named: Vec<String> = entries
                            .iter()
                            .filter(|entry| {
                                entry.get("type").and_then(Llsd::as_i32) == Some(i32::from(AT_LINK))
                            })
                            .filter_map(|entry| entry.get("asset_id").and_then(llsd_uuid))
                            .map(|target| self.role(target))
                            .filter(|role| *role != "other")
                            .map(str::to_owned)
                            .collect();
                        named.sort_unstable();
                        format!("{} entries; links to {}", entries.len(), named.join(", "))
                    },
                ),
        );
        Ok(())
    }

    /// The census roles of `slot` in a refused bake's dump.
    fn listed_order(&self, answer: Option<&Llsd>, slot: WearableType) -> String {
        let Some(answer) = answer else {
            return "no answer".to_owned();
        };
        let Some(lists) = answer
            .get("debug_cof")
            .and_then(|debug| debug.get("wearables"))
            .and_then(Llsd::as_array)
        else {
            return "no wearables list".to_owned();
        };
        let Some(list) = lists
            .get(usize::from(slot.to_code()))
            .and_then(Llsd::as_array)
        else {
            return "no list for the slot".to_owned();
        };
        let layer_asset = self
            .roles
            .iter()
            .find(|(_id, role)| *role == "the layer")
            .and_then(|(id, _role)| self.items.get(id))
            .map(|item| item.asset_id);
        let mut tokens = 0_usize;
        let roles: Vec<String> = list
            .iter()
            .map(|item| {
                let id = item.get("item_id").and_then(llsd_uuid).unwrap_or_default();
                let asset = item.get("asset_id").and_then(llsd_uuid);
                if item
                    .get("desc")
                    .and_then(Llsd::as_str)
                    .is_some_and(is_order_token)
                {
                    tokens = tokens.saturating_add(1);
                }
                let role = self.role(id);
                if role != "other" {
                    role.to_owned()
                } else if let Some(link) = self
                    .last
                    .entries
                    .iter()
                    .find(|entry| entry.item_id.uuid() == id)
                {
                    format!("a link to {}", self.role(link.target))
                } else if asset.is_some() && asset == layer_asset {
                    "the layer's asset".to_owned()
                } else {
                    "other".to_owned()
                }
            })
            .collect();
        let described = if tokens == list.len() {
            "every one described by its link's token"
        } else if tokens == 0 {
            "none described by a token"
        } else {
            "some described by a token"
        };
        format!("{}; {described}", roles.join(", "))
    }

    /// Asks for a bake as the reference does: the version a refusal names
    /// (recorded against the folder's), then `IncrementCOFVersion` and a bake
    /// at the version it answers.
    async fn bake_current(&mut self, metrics: &mut Metrics, key: &str) -> Result<(), TestFailure> {
        if self.a.cap(BAKE_CAP).is_none() {
            metrics.set(key, NO_CAPABILITY);
            metrics.set(&format!("{key}_appearance"), NO_CAPABILITY);
            return Ok(());
        }
        let stale = self
            .bake(0)
            .await
            .and_then(|answer| answer.get("expected").and_then(Llsd::as_i32));
        metrics.set(
            &format!("{key}_before_increment"),
            stale.map_or_else(
                || "no version named".to_owned(),
                |stale| format!("the folder's version {}", step(self.last.version, stale)),
            ),
        );
        let expected = self.increment().await?;
        let Some(expected) = expected else {
            metrics.set(key, "no version named");
            metrics.set(&format!("{key}_appearance"), "no bake");
            return Ok(());
        };
        let answer = self.bake(expected).await;
        metrics.set(
            key,
            format!(
                "{} at the folder's version {}",
                describe_bake(answer.as_ref(), expected),
                step(self.last.version, expected)
            ),
        );
        let mut told = Told::default();
        let (actor, window) = (self.actor, self.window);
        listen(self.a, &mut told, actor, window).await?;
        metrics.set(&format!("{key}_appearance"), told.appearances > 0);
        Ok(())
    }

    /// Copies `item` as `name` into its own folder, returning the copy.
    async fn copy(
        &mut self,
        item: &InventoryItem,
        name: &str,
    ) -> Result<Option<InventoryItem>, TestFailure> {
        let mut told = Told::default();
        self.a
            .send(Command::CopyInventoryItem {
                old_agent_id: sl_client_tokio::AgentKey::from(self.actor),
                old_item_id: item.item_id,
                new_folder_id: item.folder_id,
                new_name: name.to_owned(),
            })
            .await?;
        let actor = self.actor;
        listen_until(self.a, &mut told, actor, STEP, |told| {
            told.created
                .iter()
                .chain(told.bulk.iter())
                .any(|item| item.name == name)
        })
        .await?;
        let copy = told
            .created
            .iter()
            .chain(told.bulk.iter())
            .find(|item| item.name == name)
            .cloned();
        if let Some(copy) = copy.as_ref() {
            self.residue.copies.push(copy.item_id);
            let _previous = self.items.insert(copy.item_id.uuid(), copy.clone());
        }
        Ok(copy)
    }
}

/// A bake answer against the version asked: accepted, or refused with the
/// version it expected instead.
fn describe_bake(answer: Option<&Llsd>, asked: i32) -> String {
    let Some(answer) = answer else {
        return "no answer".to_owned();
    };
    if matches!(answer.get("success"), Some(Llsd::Boolean(true))) {
        return "accepted".to_owned();
    }
    let error = answer.get("error").and_then(Llsd::as_str).map_or_else(
        || "no error".to_owned(),
        |error| format!("\"{}\"", without_ids(error)),
    );
    answer.get("expected").and_then(Llsd::as_i32).map_or_else(
        || format!("refused ({error})"),
        |expected| {
            format!(
                "refused ({error}), expecting the version asked {}",
                step(asked, expected)
            )
        },
    )
}

/// The step from `before` to `after`, signed.
fn step(before: i32, after: i32) -> String {
    format!("{:+}", i64::from(after).saturating_sub(i64::from(before)))
}

/// A description read back against the one written.
fn describe_kept(read: &str, written: &str) -> String {
    if read == written {
        "as written".to_owned()
    } else if read.is_empty() {
        "empty".to_owned()
    } else {
        format!("\"{}\"", without_ids(read))
    }
}

/// An LLSD uuid or uuid string.
fn llsd_uuid(value: &Llsd) -> Option<Uuid> {
    match value {
        Llsd::Uuid(id) => Some(*id),
        Llsd::String(text) => Uuid::parse_str(text.trim()).ok(),
        _ => None,
    }
}

/// The first link an AIS3 answer embeds.
fn first_link(body: &Llsd) -> Option<&Llsd> {
    body.get("_embedded")
        .and_then(|embedded| embedded.get("links"))
        .and_then(Llsd::as_map)
        .and_then(|links| links.values().next())
}

/// A map's keys, sorted and space-joined.
fn key_list(value: &Llsd) -> String {
    let mut keys: Vec<&str> = value
        .as_map()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    keys.sort_unstable();
    keys.join(" ")
}

/// The reference's order token for the `index`th layer of `slot`
/// (`build_order_string`: `@` and `type * 100 + index`).
fn order_token(slot: WearableType, index: u32) -> String {
    format!(
        "@{}",
        u32::from(slot.to_code())
            .saturating_mul(100)
            .saturating_add(index)
    )
}

/// Whether `description` is an order token.
fn is_order_token(description: &str) -> bool {
    description
        .strip_prefix('@')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
}

/// Every folder the session holds, from its own model.
async fn all_folders(session: &mut Session) -> Result<Vec<FolderInfo>, TestFailure> {
    session.send(Command::QueryInventoryFolders).await?;
    session
        .wait_for(STEP, |event| match event {
            Event::InventoryFolders(folders) => Some(folders.to_vec()),
            _ => None,
        })
        .await
}

/// Every item of `folders`, read one folder at a time.
async fn all_items(
    session: &mut Session,
    folders: &[FolderInfo],
) -> Result<HashMap<Uuid, InventoryItem>, TestFailure> {
    let mut items = HashMap::new();
    for folder in folders {
        let wanted = folder.folder_id;
        session.send(Command::RequestFolderContents(wanted)).await?;
        let listed = session
            .wait_for(STEP, |event| match event {
                Event::InventoryDescendents {
                    folder_id, items, ..
                } if *folder_id == wanted => Some(items.clone()),
                _ => None,
            })
            .await?;
        for item in listed {
            let _previous = items.insert(item.item_id.uuid(), item);
        }
    }
    Ok(items)
}

/// The census.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let a = ctx.primary();
    let actor = a
        .agent_id()
        .map(|agent| agent.uuid())
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let session_id = a
        .session_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;

    // --- 1. The arrival. ---
    let login_cof = a.login_success().and_then(|login| login.cof_version);
    metrics.set("arrival_login_cof_version", login_cof.is_some());
    let mut arrival = Told::default();
    a.wait_for(REGION_TIMEOUT, |event| {
        arrival.note(actor, event);
        matches!(
            event,
            Event::RegionHandshakeComplete | Event::RegionChanged { .. }
        )
        .then_some(())
    })
    .await?;
    listen(a, &mut arrival, actor, ARRIVAL_WINDOW).await?;
    let skeleton = arrival.skeleton.clone().unwrap_or_default();
    let skeleton_cof = skeleton
        .iter()
        .find(|folder| folder.folder_type == FolderType::CurrentOutfit.to_code());
    metrics.set(
        "arrival_skeleton_cof",
        skeleton_cof.map_or_else(
            || {
                if arrival.skeleton.is_some() {
                    "absent"
                } else {
                    "no skeleton heard"
                }
                .to_owned()
            },
            |folder| match login_cof {
                Some(login) if login == folder.version => {
                    "listed, at the login's version".to_owned()
                }
                Some(_) => "listed, at another version than the login's".to_owned(),
                None => "listed".to_owned(),
            },
        ),
    );

    let folders = all_folders(a).await?;
    let cof = folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::CurrentOutfit)
        .map(|folder| folder.folder_id)
        .ok_or_else(|| TestFailure::Assertion("no Current Outfit Folder".to_owned()))?;
    let root = folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::RootInventory)
        .map(|folder| folder.folder_id)
        .ok_or_else(|| TestFailure::Assertion("no inventory root".to_owned()))?;
    let trash = folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::Trash)
        .map(|folder| folder.folder_id)
        .ok_or_else(|| TestFailure::Assertion("no Trash".to_owned()))?;
    let leftover_folders: Vec<InventoryFolderKey> = folders
        .iter()
        .filter(|folder| folder.name.starts_with(PREFIX))
        .map(|folder| folder.folder_id)
        .collect();
    let items = all_items(a, &folders).await?;
    let http = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;
    let mut census = Census {
        a,
        actor,
        session_id,
        window: WINDOW,
        http,
        cof,
        root,
        trash,
        items,
        roles: Vec::new(),
        residue: Residue::default(),
        last: CofRead::default(),
    };

    // A previous run's leftovers: its links, its copies, its folders.
    let first = census.read_cof().await?;
    let leftovers: Vec<InventoryKey> = census
        .items
        .values()
        .filter(|item| item.name.starts_with(PREFIX))
        .map(|item| item.item_id)
        .collect();
    let leftover_links: Vec<InventoryKey> = first
        .entries
        .iter()
        .filter(|entry| {
            leftovers.iter().any(|item| item.uuid() == entry.target)
                || leftover_folders
                    .iter()
                    .any(|folder| folder.uuid() == entry.target)
        })
        .map(|entry| entry.item_id)
        .collect();
    if !leftovers.is_empty() || !leftover_folders.is_empty() {
        tracing::warn!(
            items = leftovers.len(),
            links = leftover_links.len(),
            folders = leftover_folders.len(),
            "removing a previous run's leftovers"
        );
        // A run that ended before its restore may have left the shape's copy
        // as the folder's only shape: wear the item it was copied from again.
        let shape_slot = Some(WearableType::Shape);
        let shapes_left: Vec<&Entry> = first
            .entries
            .iter()
            .filter(|entry| {
                entry.link_type == AT_LINK && census.slot_of(entry.target) == shape_slot
            })
            .collect();
        let original = if !shapes_left.is_empty()
            && shapes_left
                .iter()
                .all(|entry| leftovers.iter().any(|item| item.uuid() == entry.target))
        {
            let asset = shapes_left
                .first()
                .and_then(|entry| census.items.get(&entry.target))
                .map(|copy| copy.asset_id);
            census
                .items
                .values()
                .find(|item| {
                    !item.name.starts_with(PREFIX)
                        && Some(item.asset_id) == asset
                        && census.slot_of(item.item_id.uuid()) == shape_slot
                })
                .cloned()
        } else {
            None
        };
        census.residue.shape_unlinked = original.is_some();
        census.residue.links = leftover_links;
        census.residue.copies = leftovers;
        census.residue.folders = leftover_folders;
        put_back(&mut census, metrics, original.as_ref(), None, None, false).await;
        census.read_cof().await?;
    }
    census.record_told(metrics, "arrival", &arrival);

    let mut restore = Restore::default();
    let outcome = legs(&mut census, metrics, login_cof, &mut restore).await;
    let cleanup_metrics = &mut Metrics::new();
    put_back(
        &mut census,
        cleanup_metrics,
        restore.original_shape.as_ref(),
        restore.arrival_set,
        restore.arrival_targets,
        true,
    )
    .await;
    metrics.merge(cleanup_metrics.clone());
    outcome
}

/// What the restore needs from the legs.
#[derive(Debug, Clone, Default)]
struct Restore {
    /// The shape A arrived wearing.
    original_shape: Option<InventoryItem>,
    /// The wearables A wears, as the folder states them, to state again.
    arrival_set: Option<Vec<Wearable>>,
    /// The COF's link targets at the arrival.
    arrival_targets: Option<Vec<Uuid>>,
}

/// The legs, noting in `restore` what the restore needs as they learn it, so
/// that a leg that fails still leaves the outfit to be put back.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    login_cof: Option<i32>,
    restore: &mut Restore,
) -> Result<(), TestFailure> {
    // --- 2. The folder. ---
    let read = census.read_cof().await?;
    metrics.set(
        "cof_version_against_login",
        login_cof.map_or_else(
            || "no login version".to_owned(),
            |login| step(login, read.version),
        ),
    );
    let mut kinds: HashMap<&'static str, i64> = HashMap::new();
    let mut unresolved = 0_i64;
    let mut not_links = 0_i64;
    let mut ordered = 0_i64;
    let mut clothing = 0_i64;
    let mut other_descriptions: Vec<String> = Vec::new();
    for entry in &read.entries {
        if entry.link_type != AT_LINK && entry.link_type != AT_LINK_FOLDER {
            not_links = not_links.saturating_add(1);
            continue;
        }
        if entry.link_type == AT_LINK_FOLDER {
            let count = kinds.entry("folder link").or_default();
            *count = count.saturating_add(1);
            continue;
        }
        let Some(target) = census.items.get(&entry.target) else {
            unresolved = unresolved.saturating_add(1);
            continue;
        };
        let kind = match AssetType::from_code(i32::from(target.item_type)) {
            AssetType::Bodypart => "body part",
            AssetType::Clothing => "clothing",
            AssetType::Object => "object",
            AssetType::Gesture => "gesture",
            _ => "other",
        };
        let count = kinds.entry(kind).or_default();
        *count = count.saturating_add(1);
        if kind == "clothing" {
            clothing = clothing.saturating_add(1);
            if is_order_token(&entry.description) {
                ordered = ordered.saturating_add(1);
            }
        } else if !entry.description.is_empty() {
            other_descriptions.push(format!("{kind}: {}", describe_kept(&entry.description, "")));
        }
    }
    metrics.set("cof_unresolved", unresolved);
    metrics.set("cof_not_links", not_links);
    metrics.set(
        "cof_listing_carries",
        if read.carried == 0 {
            "the folder's entries only".to_owned()
        } else if read.carried == read.entries.len() {
            "every link's target too".to_owned()
        } else {
            format!("{} items of other folders", read.carried)
        },
    );
    metrics.set(
        "cof_body_parts",
        kinds.get("body part").copied().unwrap_or(0),
    );
    let mut kind_words: Vec<&str> = kinds.keys().copied().collect();
    kind_words.sort_unstable();
    metrics.set("cof_kinds", kind_words.join(", "));
    metrics.set(
        "cof_clothing_order",
        if clothing == 0 {
            "no clothing".to_owned()
        } else if ordered == clothing {
            "every clothing link ordered".to_owned()
        } else if ordered == 0 {
            "no clothing link ordered".to_owned()
        } else {
            format!("{ordered} of {clothing} ordered")
        },
    );
    other_descriptions.sort_unstable();
    other_descriptions.dedup();
    metrics.set(
        "cof_other_descriptions",
        if other_descriptions.is_empty() {
            "empty".to_owned()
        } else {
            other_descriptions.join(" | ")
        },
    );
    let raw = census
        .raw(
            reqwest::Method::GET,
            &sl_wire::ais_category_children_fetch_url(census.cof, 0),
            None,
        )
        .await;
    metrics.set("cof_ais_status", raw.status_word());
    metrics.set(
        "cof_ais_link_keys",
        raw.body.as_ref().and_then(first_link).map_or_else(
            || {
                raw.status
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "no link".to_owned())
            },
            key_list,
        ),
    );

    // The worn wearables, as the folder states them.
    let worn: Vec<(Uuid, WearableType)> = read
        .entries
        .iter()
        .filter(|entry| entry.link_type == AT_LINK)
        .filter_map(|entry| {
            census
                .slot_of(entry.target)
                .map(|slot| (entry.target, slot))
        })
        .collect();
    restore.arrival_targets = Some(read.entries.iter().map(|entry| entry.target).collect());
    let shape = worn
        .iter()
        .find(|(_id, slot)| *slot == WearableType::Shape)
        .and_then(|(id, _slot)| census.items.get(id).cloned());
    restore.original_shape.clone_from(&shape);
    if let Some(shape) = shape.as_ref() {
        census.roles.push((shape.item_id.uuid(), "the shape"));
    }
    // The clothing layer of the lowest slot, so that every run picks the
    // same one whatever order the folder is listed in.
    let layer = worn
        .iter()
        .filter(|(_id, slot)| !slot.is_body_part())
        .min_by_key(|(id, slot)| (slot.to_code(), *id))
        .and_then(|(id, _slot)| census.items.get(id).cloned());
    if let Some(layer) = layer.as_ref() {
        census.roles.push((layer.item_id.uuid(), "the layer"));
    }

    // --- 3. The legacy reply. ---
    let reply = census.wearables(metrics, "wearables").await?;
    let folder_set: Vec<Wearable> = worn
        .iter()
        .map(|(id, slot)| Wearable {
            item_id: InventoryKey::from(*id),
            asset_id: None,
            wearable_type: *slot,
        })
        .collect();
    metrics.set(
        "wearables_against_cof",
        reply.as_ref().map_or_else(
            || "no reply".to_owned(),
            |reply| {
                let mut replied: Vec<Uuid> = reply.iter().map(|worn| worn.item_id.uuid()).collect();
                let mut folder: Vec<Uuid> =
                    folder_set.iter().map(|worn| worn.item_id.uuid()).collect();
                replied.sort_unstable();
                folder.sort_unstable();
                if replied == folder {
                    "the folder's wearables".to_owned()
                } else if replied.iter().all(|id| folder.contains(id)) {
                    "some of the folder's wearables".to_owned()
                } else {
                    "other items".to_owned()
                }
            },
        ),
    );
    // What the restore states again: the folder's set on a grid whose reply
    // is not the outfit, the reply where it is.
    restore.arrival_set = Some(match reply.as_ref() {
        Some(reply) if is_opensim_reply(reply, &folder_set) => reply.clone(),
        _ => folder_set.clone(),
    });

    // --- 4. Wearing by the folder. ---
    let Some(shape) = shape else {
        return Err(TestFailure::Assertion(
            "the Current Outfit Folder links no shape".to_owned(),
        ));
    };
    let copy = census.copy(&shape, SHAPE_COPY).await?;
    metrics.set("copy_shape", copy.is_some());
    let Some(copy) = copy else {
        return Err(TestFailure::Assertion(
            "the shape could not be copied".to_owned(),
        ));
    };
    census.roles.push((copy.item_id.uuid(), "the shape's copy"));
    let _link = census
        .link(
            metrics,
            "wear_link",
            InventoryItemOrFolderKey::Item(copy.item_id),
            InventoryType::Wearable,
            "",
        )
        .await?;
    let original_link = census
        .last
        .link_to(shape.item_id.uuid())
        .map(|entry| entry.item_id);
    if let Some(link) = original_link {
        census.unlink(metrics, "wear_unlink", link).await?;
        census.residue.shape_unlinked = true;
    } else {
        metrics.set("wear_unlink_answer", "no link to the shape");
    }
    let _reply = census.wearables(metrics, "wear_wearables").await?;
    census.bake_current(metrics, "wear_bake").await?;

    // --- 5. A UDP link into the folder. ---
    let before = census.last.version;
    let mut told = Told::default();
    let message = AnyMessage::LinkInventoryItem(LinkInventoryItem {
        agent_data: LinkInventoryItemAgentDataBlock {
            agent_id: census.actor,
            session_id: census.session_id,
        },
        inventory_block: LinkInventoryItemInventoryBlockBlock {
            callback_id: 7,
            folder_id: census.cof.uuid(),
            transaction_id: Uuid::nil(),
            old_item_id: shape.item_id.uuid(),
            r#type: AT_LINK,
            inv_type: i8::try_from(InventoryType::Wearable.to_code()).unwrap_or(-1),
            name: nul_terminated(&shape.name),
            description: nul_terminated(""),
        },
    });
    census
        .a
        .send(Command::Send {
            message: Box::new(message),
            reliability: Reliability::Reliable,
        })
        .await?;
    let actor = census.actor;
    let window = census.window;
    listen(census.a, &mut told, actor, window).await?;
    census.record_told(metrics, "udp_link", &told);
    let read = census.read_cof().await?;
    metrics.set("udp_link_version_step", step(before, read.version));
    let udp_link = read
        .link_to(shape.item_id.uuid())
        .map(|entry| entry.item_id);
    metrics.set("udp_link_listed", udp_link.is_some());
    if let Some(link) = udp_link {
        census.residue.links.push(link);
        census.unlink(metrics, "udp_unlink", link).await?;
    } else {
        for suffix in [
            "alerts",
            "answer",
            "appearance",
            "gone",
            "inventory",
            "version_step",
            "versions",
            "wearables_update",
        ] {
            metrics.set(&format!("udp_unlink_{suffix}"), "no link made");
        }
    }

    // --- 6. Wearing by AgentIsNowWearing. ---
    let worn_copy: Vec<Wearable> = folder_set
        .iter()
        .map(|worn| {
            if worn.wearable_type == WearableType::Shape {
                Wearable {
                    item_id: copy.item_id,
                    asset_id: None,
                    wearable_type: WearableType::Shape,
                }
            } else {
                *worn
            }
        })
        .collect();
    census.residue.wearing_changed = true;
    census
        .now_wearing(metrics, "now_wearing", worn_copy.clone())
        .await?;
    let _reply = census.wearables(metrics, "now_wearing_wearables").await?;

    // --- 7. A new login. ---
    let previous = census.last.version;
    census.a.relogin().await?;
    let relogin_cof = census.a.login_success().and_then(|login| login.cof_version);
    let mut told = Told::default();
    census
        .a
        .wait_for(REGION_TIMEOUT, |event| {
            told.note(actor, event);
            matches!(
                event,
                Event::RegionHandshakeComplete | Event::RegionChanged { .. }
            )
            .then_some(())
        })
        .await?;
    listen(census.a, &mut told, actor, ARRIVAL_WINDOW).await?;
    census.record_told(metrics, "relogin", &told);
    metrics.set(
        "relogin_login_cof_version",
        relogin_cof.map_or_else(|| "none".to_owned(), |version| step(previous, version)),
    );
    let read = census.read_cof().await?;
    metrics.set("relogin_cof_version_step", step(previous, read.version));
    metrics.set(
        "relogin_cof_shape",
        match (
            read.link_to(copy.item_id.uuid()).is_some(),
            read.link_to(shape.item_id.uuid()).is_some(),
        ) {
            (true, false) => "the copy",
            (false, true) => "the shape",
            (true, true) => "both",
            (false, false) => "neither",
        },
    );
    let _reply = census.wearables(metrics, "relogin_wearables").await?;

    // --- 8. Layer order. ---
    match layer {
        Some(layer) => order(census, metrics, &layer, &worn_copy).await?,
        None => {
            for key in [
                "order_a_description",
                "order_b_description",
                "order_bake",
                "order_bake_baked",
                "order_bake_cof_raw",
                "order_bake_increment",
                "order_reply",
                "order_swapped_bake",
                "order_swapped_bake_baked",
                "order_swapped_bake_cof_raw",
                "order_swapped_bake_increment",
                "order_swapped_reply",
            ] {
                metrics.set(key, "no clothing worn");
            }
        }
    }

    // --- 9. An outfit folder. ---
    let folder = make_outfit_folder(census, metrics).await?;
    if let Some(folder) = folder {
        census.roles.push((folder.uuid(), "the outfit folder"));
        let link = census
            .link(
                metrics,
                "outfit_link",
                InventoryItemOrFolderKey::Folder(folder),
                InventoryType::Category,
                "",
            )
            .await?;
        if let Some(link) = link {
            census.unlink(metrics, "outfit_unlink", link).await?;
        }
    }
    Ok(())
}

/// Whether `reply` is the outfit itself: the folder's set, by item.
fn is_opensim_reply(reply: &[Wearable], folder: &[Wearable]) -> bool {
    !reply.is_empty()
        && reply
            .iter()
            .all(|worn| folder.iter().any(|known| known.item_id == worn.item_id))
}

/// The layer-order leg: two copies of `layer` linked behind it with the
/// reference's order tokens, then with the tokens swapped.
async fn order(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    layer: &InventoryItem,
    worn: &[Wearable],
) -> Result<(), TestFailure> {
    let Some(slot) = census.slot_of(layer.item_id.uuid()) else {
        metrics.set("order_a_description", "the layer has no slot");
        return Ok(());
    };
    let first = census.copy(layer, LAYER_A).await?;
    let second = census.copy(layer, LAYER_B).await?;
    let (Some(first), Some(second)) = (first, second) else {
        metrics.set("order_a_description", "the layer could not be copied");
        return Ok(());
    };
    census.roles.push((first.item_id.uuid(), "layer a"));
    census.roles.push((second.item_id.uuid(), "layer b"));
    let behind = u32::try_from(
        census
            .last
            .entries
            .iter()
            .filter(|entry| census.slot_of(entry.target) == Some(slot))
            .count(),
    )
    .unwrap_or(0);
    metrics.set("order_slot_links_before", i64::from(behind));
    metrics.set("order_slot", format!("{slot:?}"));
    let (token_a, token_b) = (
        order_token(slot, behind),
        order_token(slot, behind.saturating_add(1)),
    );
    let link_a = census
        .link(
            metrics,
            "order_a",
            InventoryItemOrFolderKey::Item(first.item_id),
            InventoryType::Wearable,
            &token_a,
        )
        .await?;
    let link_b = census
        .link(
            metrics,
            "order_b",
            InventoryItemOrFolderKey::Item(second.item_id),
            InventoryType::Wearable,
            &token_b,
        )
        .await?;
    census.bake_order(metrics, "order_bake", slot).await?;
    let mut set: Vec<Wearable> = worn.to_vec();
    for item in [&first, &second] {
        set.push(Wearable {
            item_id: item.item_id,
            asset_id: None,
            wearable_type: slot,
        });
    }
    census
        .now_wearing(metrics, "order_wearing", set.clone())
        .await?;
    let reply = census.wearables(metrics, "order_wearables").await?;
    metrics.set("order_reply", slot_roles(census, reply.as_deref(), slot));

    if let (Some(link_a), Some(link_b)) = (link_a, link_b) {
        census
            .describe(metrics, "order_swap_a", link_a, &token_b)
            .await?;
        census
            .describe(metrics, "order_swap_b", link_b, &token_a)
            .await?;
    }
    census
        .bake_order(metrics, "order_swapped_bake", slot)
        .await?;
    let swapped: Vec<Wearable> = worn
        .iter()
        .copied()
        .chain([&second, &first].into_iter().map(|item| Wearable {
            item_id: item.item_id,
            asset_id: None,
            wearable_type: slot,
        }))
        .collect();
    census
        .now_wearing(metrics, "order_swapped_wearing", swapped)
        .await?;
    let reply = census.wearables(metrics, "order_swapped_wearables").await?;
    metrics.set(
        "order_swapped_reply",
        slot_roles(census, reply.as_deref(), slot),
    );
    for (key, link) in [("order_unlink_a", link_a), ("order_unlink_b", link_b)] {
        if let Some(link) = link {
            census.unlink(metrics, key, link).await?;
        }
    }
    Ok(())
}

/// The census roles a wearables reply lists on `slot`, in order.
fn slot_roles(census: &Census<'_>, reply: Option<&[Wearable]>, slot: WearableType) -> String {
    reply.map_or_else(
        || "no reply".to_owned(),
        |reply| {
            let roles: Vec<&str> = reply
                .iter()
                .filter(|worn| worn.wearable_type == slot)
                .map(|worn| census.role(worn.item_id.uuid()))
                .collect();
            if roles.is_empty() {
                "nothing on the slot".to_owned()
            } else {
                roles.join(", ")
            }
        },
    )
}

/// Makes an outfit folder under the root: over AIS3 where granted, over UDP
/// otherwise. Returns its id where the grid lists it.
async fn make_outfit_folder(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
) -> Result<Option<InventoryFolderKey>, TestFailure> {
    let folder = if census.ais().is_some() {
        let raw = census
            .raw(
                reqwest::Method::POST,
                &sl_wire::ais_create_category_url(census.root, Uuid::new_v4()),
                Some(sl_wire::build_ais_create_category_body(
                    census.root,
                    i32::from(FolderType::Outfit.to_code()),
                    OUTFIT_FOLDER,
                )),
            )
            .await;
        metrics.set(
            "outfit_folder_answer",
            format!("ais3 {}", raw.status_word()),
        );
        raw.body
            .as_ref()
            .and_then(|body| body.get("_created_categories"))
            .and_then(Llsd::as_array)
            .and_then(|created| created.first())
            .and_then(llsd_uuid)
            .map(InventoryFolderKey::from)
    } else {
        let folder = InventoryFolderKey::from(Uuid::new_v4());
        census
            .a
            .send(Command::CreateInventoryFolder {
                folder_id: folder,
                parent_id: census.root,
                folder_type: FolderType::Outfit,
                name: OUTFIT_FOLDER.to_owned(),
            })
            .await?;
        metrics.set("outfit_folder_answer", "udp sent");
        let mut told = Told::default();
        let (actor, window) = (census.actor, census.window);
        listen(census.a, &mut told, actor, window).await?;
        Some(folder)
    };
    if let Some(folder) = folder {
        census.residue.folders.push(folder);
    }
    metrics.set("outfit_folder_made", folder.is_some());
    Ok(folder)
}

/// A string as the message template carries it: NUL-terminated.
fn nul_terminated(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Puts everything back: the census's links out of the COF, the shape's
/// link in again, the arrival's outfit stated, the copies and folders
/// removed and, where the grid bakes, a bake at the folder's version.
/// Records under `restore_*` when `record` is set; never fails, but logs.
async fn put_back(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    shape: Option<&InventoryItem>,
    arrival_set: Option<Vec<Wearable>>,
    arrival_targets: Option<Vec<Uuid>>,
    record: bool,
) {
    let outcome =
        put_back_inner(census, metrics, shape, arrival_set, arrival_targets, record).await;
    if let Err(failure) = outcome {
        tracing::error!(%failure, "putting the outfit back failed");
        if record {
            metrics.set("restore_failed", failure.to_string());
        }
    }
}

/// [`put_back`] proper.
async fn put_back_inner(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    shape: Option<&InventoryItem>,
    arrival_set: Option<Vec<Wearable>>,
    arrival_targets: Option<Vec<Uuid>>,
    record: bool,
) -> Result<(), TestFailure> {
    let scratch = &mut Metrics::new();
    for link in std::mem::take(&mut census.residue.links) {
        census.unlink(scratch, "restore_unlink", link).await?;
    }
    if census.residue.shape_unlinked
        && let Some(shape) = shape
    {
        let _link = census
            .link(
                scratch,
                "restore_link",
                InventoryItemOrFolderKey::Item(shape.item_id),
                InventoryType::Wearable,
                "",
            )
            .await?;
        // The shape's own link is the outfit's, not the census's.
        census.residue.links.clear();
        census.residue.shape_unlinked = false;
    }
    if census.residue.wearing_changed
        && let Some(set) = arrival_set
    {
        census.now_wearing(scratch, "restore_wearing", set).await?;
        census.residue.wearing_changed = false;
    }
    let copies = std::mem::take(&mut census.residue.copies);
    if !copies.is_empty() {
        census.a.send(Command::RemoveInventoryItems(copies)).await?;
    }
    let folders = std::mem::take(&mut census.residue.folders);
    for folder in &folders {
        census
            .a
            .send(Command::MoveInventoryFolder {
                folder_id: *folder,
                parent_id: census.trash,
            })
            .await?;
    }
    if !folders.is_empty() {
        census
            .a
            .send(Command::RemoveInventoryFolders(folders))
            .await?;
    }
    let mut told = Told::default();
    let (actor, window) = (census.actor, census.window);
    listen(census.a, &mut told, actor, window).await?;
    let read = census.read_cof().await?;
    census.bake_current(scratch, "restore_bake").await?;
    if record {
        metrics.set(
            "restore_cof",
            arrival_targets.map_or_else(
                || "no arrival read".to_owned(),
                |mut arrival| {
                    let mut now: Vec<Uuid> =
                        read.entries.iter().map(|entry| entry.target).collect();
                    arrival.sort_unstable();
                    now.sort_unstable();
                    if arrival == now {
                        "as at the arrival".to_owned()
                    } else {
                        "changed".to_owned()
                    }
                },
            ),
        );
        if let Some(bake) = scratch.get("restore_bake") {
            metrics.set("restore_bake", format!("{bake:?}"));
        }
    }
    Ok(())
}
