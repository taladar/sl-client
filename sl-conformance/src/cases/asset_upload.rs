//! Upload a new asset over the modern CAPS `NewFileAgentInventory` uploader and
//! confirm the two-step flow both stores the asset *and* creates the inventory
//! item — the outcome the modern viewer relies on.
//!
//! The uploader is a two-step CAPS POST: the LLSD metadata (destination folder,
//! asset/inventory class, permissions, expected cost) goes to the
//! `NewFileAgentInventory` capability, which answers with an `uploader` URL; the
//! raw asset bytes go there, and the completion carries the new asset UUID plus
//! the new inventory-item UUID. Both grids offer the capability and the modern
//! viewer uploads exclusively over it, so the legacy UDP `AssetUploadRequest`
//! path is not exercised (it was dropped in favour of this CAPS-only flow,
//! mirroring `asset-fetch-http`).
//!
//! # Two grids, two asset classes, and why
//!
//! **OpenSim** serves notecard creation through `NewFileAgentInventory`, and a
//! notecard needs no client-side encoding — the bytes are the `Linden text
//! version 2` container a viewer POSTs verbatim — so that is what is uploaded
//! there, free of charge.
//!
//! **Second Life does not.** Its `NewFileAgentInventory` accepts only the
//! chargeable file-upload classes (texture, sound, animation, mesh, …) and
//! answers a notecard with `Invalid asset type`; a notecard is instead created
//! empty (`CreateInventoryItem`) and its body set with
//! `UpdateNotecardAgentInventory`, which `notecard-create-update` measures. So
//! on Second Life this case uploads the cheapest class the capability will
//! take: a small JPEG-2000 texture, at the account's own upload price.
//!
//! That price is **not** [`Event::EconomyData`]'s `price_upload`. On Second Life
//! the reference viewer charges from the login response's benefits package
//! (`LLAgentBenefits::getTextureUploadCost`) and falls back to the older field
//! only off Second Life, and the benefits package prices a texture *by area* —
//! `large_texture_upload_cost` above `MIN_2K_TEXTURE_AREA`, which one flat
//! `price_upload` cannot express. The grid checks the `expected_upload_cost` it
//! is sent, so a case that sent the legacy number would be refused. The texture
//! is deliberately far below the 2K tier, so the flat rate is the right one and
//! the run is as cheap as the grid allows.
//!
//! # The `upload_announcement` measurement
//!
//! What a grid pushes after an upload completes *besides* the HTTP response is
//! the question `sl_fake_grid::UploadAnnouncements` answers, so the completion is
//! watched with [`observe_upload`] rather than merely awaited and the shapes
//! naming the new item are recorded as `upload_announcement`
//! (`update-create-inventory-item`, `bulk-update-inventory`, or `none`).
//!
//! Both halves of it are measured here, and they agree: **neither grid
//! announces an item this capability created** (`none` on OpenSim, and `none`
//! on aditi over two paid runs). That is not what
//! `test-fake-grid-imitates-upload-announcements` extrapolated from the
//! neighbouring path — it expected the legacy push, Second Life having been
//! measured sending exactly that after an in-place save — which is why the
//! run this case now performs was worth its fee. The in-place save is measured
//! separately by `notecard-create-update`, and the two together are what
//! `sl_fake_grid::UploadAnnouncements` carries a value per path for.

use sl_client_tokio::{
    AssetType, Command, Event, FolderInfo, FolderType, InventoryFolderKey, InventoryKey,
    InventoryType, MoneyBalance, Throttle, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, is_aditi, observe_upload};

/// The edge of the texture uploaded to a grid that charges for one.
///
/// Small on purpose. The benefits package prices a texture by area and anything
/// above 1024×1024 crosses into `large_texture_upload_cost` (L$ 50 on aditi
/// against L$ 10 flat), so a case that uploaded a large texture would spend five
/// times as much of the test avatar's balance to measure the same thing. 64×64
/// is also a plausible real upload rather than a degenerate one.
const TEXTURE_EDGE: u32 = 64;

/// The checker cell of that texture, in texels.
const TEXTURE_CELL: u32 = 8;

/// Uploads an asset over the `NewFileAgentInventory` capability.
#[derive(Debug)]
pub struct AssetUpload;

impl GridTest for AssetUpload {
    fn name(&self) -> &'static str {
        "asset-upload"
    }

    fn description(&self) -> &'static str {
        "Upload an asset over the NewFileAgentInventory CAPS uploader"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The two-step CAPS uploader both grids offer; without it the modern
            // upload cannot run (the legacy UDP path was dropped).
            if session.cap("NewFileAgentInventory").is_none() {
                ctx.mark_partial("no NewFileAgentInventory capability offered");
                return Ok(());
            }

            // A distinct per-run name so a leftover item from an aborted run
            // cannot be confused for this run's, and concurrent runs never
            // collide.
            let tag: String = Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let name = format!("conf-upload-{tag}");

            // What this grid's uploader will take, and what it charges for it.
            let subject = Subject::for_grid(grid, &tag)?;
            let cost = match subject.price(ctx.primary()) {
                Ok(cost) => cost,
                Err(reason) => {
                    ctx.mark_partial(&reason);
                    return Ok(());
                }
            };
            let byte_len = subject.data.len();

            // Upload into the system folder for the class, as a viewer does;
            // the inventory root is the fallback when the grid named no such
            // folder in the login skeleton.
            let Some(folder) = destination_folder(ctx.primary(), subject.folder_type).await? else {
                return Err(TestFailure::Assertion(
                    "no agent inventory root to upload into".to_owned(),
                ));
            };

            // A chargeable upload is spent money, so check the balance covers it
            // first and decline the run rather than reaching the grid's own
            // refusal — a case that quietly empties the test avatar's purse is
            // worse than one that says why it did not run.
            let balance_before = if cost > 0 {
                let balance = current_balance(ctx.primary()).await?;
                if balance < cost {
                    ctx.mark_partial(&format!(
                        "balance L$ {balance} will not cover the L$ {cost} upload fee"
                    ));
                    return Ok(());
                }
                Some(balance)
            } else {
                None
            };

            let session = ctx.primary();
            session
                .send(Command::UploadAsset {
                    folder_id: folder,
                    asset_type: subject.asset_type,
                    inventory_type: subject.inventory_type,
                    name: name.clone(),
                    description: "sl-conformance asset-upload".to_owned(),
                    // The next-owner mask a viewer sends for a fresh upload
                    // (move / modify / copy / transfer).
                    next_owner_mask: 0x0008_e000,
                    group_mask: 0,
                    everyone_mask: 0,
                    expected_upload_cost: i32::try_from(cost).unwrap_or(i32::MAX),
                    data: subject.data,
                })
                .await?;

            // The two-step uploader completes as `AssetUploaded` (stored asset +
            // created item) or `AssetUploadFailed` (a grid/permission error) —
            // watched rather than merely awaited, so what the grid pushes
            // *besides* the completion is recorded too (see the module docs).
            let observed = observe_upload(session, LONG_TIMEOUT).await?;
            // The completion's own round trip, not the watch's: `observe_upload`
            // keeps listening for a settle window after the upload finished, and
            // timing that in would make every recorded upload look like it took
            // the settle.
            let upload_secs = observed.elapsed.as_secs_f64();

            let completion = observed.outcome.clone().map_err(|reason| {
                TestFailure::Assertion(format!("NewFileAgentInventory upload failed: {reason}"))
            })?;
            let new_asset = completion.new_asset;
            check(!new_asset.is_nil(), "upload stored a nil asset id")?;
            let new_item = completion
                .new_inventory_item
                .filter(|item| !item.is_nil())
                .ok_or_else(|| {
                    TestFailure::Assertion("upload created no inventory item".to_owned())
                })?;
            let announcement = observed.announced_for(InventoryKey::from(new_item));

            // What the grid actually took, which is the only check on the claim
            // that the benefits package — and not `EconomyData` — is the price
            // list a Second Life upload is billed from. The balance reply the
            // charge itself pushes is eaten by the watch above, so this asks
            // again.
            let charged = match balance_before {
                Some(before) => Some(before.saturating_sub(current_balance(ctx.primary()).await?)),
                None => None,
            };

            let metrics = ctx.metrics();
            metrics.set_timing("upload_secs", upload_secs);
            metrics.set("asset_bytes", i64::try_from(byte_len).unwrap_or(-1));
            metrics.set("upload_asset_class", subject.class);
            metrics.set("upload_cost", cost);
            metrics.set("upload_cost_source", subject.cost_source);
            if let Some(charged) = charged {
                metrics.set("upload_charged", charged);
            }
            metrics.set("new_asset", new_asset.to_string());
            metrics.set("new_item", new_item.to_string());
            metrics.set("upload_announcement", announcement);

            // Best-effort cleanup: delete the created item so runs do not
            // accumulate inventory. A failure here does not fail the case — the
            // upload (the thing under test) already succeeded. The L$ is spent
            // either way; deleting the item does not refund it.
            let session = ctx.primary();
            session
                .send(Command::RemoveInventoryItems(vec![InventoryKey::from(
                    new_item,
                )]))
                .await
                .ok();

            Ok(())
        })
    }
}

/// What this grid's `NewFileAgentInventory` will accept, and how it is priced.
struct Subject {
    /// The asset class posted in the upload metadata.
    asset_type: AssetType,
    /// The inventory-item class posted with it.
    inventory_type: InventoryType,
    /// The system folder a viewer would file the new item into.
    folder_type: FolderType,
    /// The class name recorded as `upload_asset_class`, so a record says which
    /// class the grid beside it was measured on.
    class: &'static str,
    /// Where the `expected_upload_cost` comes from, recorded as
    /// `upload_cost_source`: the account's benefits package, or nothing because
    /// the class is free here.
    cost_source: &'static str,
    /// The texture's dimensions, when the subject is one — the benefits package
    /// prices a texture by area, so the cost is not a constant.
    texture_size: Option<(u32, u32)>,
    /// The raw asset bytes.
    data: Vec<u8>,
}

impl Subject {
    /// The asset to upload to `grid`, tagged with this run's `tag` where the
    /// bytes carry one.
    ///
    /// # Errors
    ///
    /// Returns an assertion failure when the fixture texture will not encode,
    /// which is a client-side fault rather than anything the grid did.
    fn for_grid(grid: Grid, tag: &str) -> Result<Self, TestFailure> {
        if is_aditi(grid) {
            // Second Life takes only the chargeable classes here; a texture is
            // the cheapest of them. The pixels are a checkerboard rather than a
            // solid so the codestream is not degenerate — a single-colour image
            // compresses to almost nothing and would test the encoder less than
            // it tests the grid's tolerance for a tiny asset.
            let image = sl_test_assets::RgbaImage::checker(
                TEXTURE_EDGE,
                TEXTURE_CELL,
                [0xFF, 0x00, 0x99, 0xFF],
                [0x11, 0x11, 0x11, 0xFF],
            );
            let data = image
                .j2c()
                .map_err(|error| TestFailure::Assertion(format!("j2c encode failed: {error}")))?;
            return Ok(Self {
                asset_type: AssetType::Texture,
                inventory_type: InventoryType::Texture,
                folder_type: FolderType::Texture,
                class: "texture",
                cost_source: "account_level_benefits",
                texture_size: Some((TEXTURE_EDGE, TEXTURE_EDGE)),
                data,
            });
        }
        Ok(Self {
            asset_type: AssetType::Notecard,
            inventory_type: InventoryType::Notecard,
            folder_type: FolderType::Notecard,
            class: "notecard",
            cost_source: "free",
            texture_size: None,
            // A distinct per-run body, so a leftover notecard is identifiable.
            data: notecard_bytes(&format!("sl-conformance asset-upload {tag}\n")),
        })
    }

    /// The L$ this upload expects to be charged, or the reason the run cannot
    /// name one.
    ///
    /// A free class costs nothing to ask about. A texture is priced from the
    /// login response's benefits package, by area — the number the grid checks
    /// the request against, and the one `EconomyData::price_upload` is *not*.
    /// A grid that sent no benefits package leaves nothing here to send, and
    /// guessing a fee onto a live account's balance is not a guess worth making.
    fn price(&self, session: &Session) -> Result<i64, String> {
        let Some((width, height)) = self.texture_size else {
            return Ok(0);
        };
        let benefits = session
            .login_account()
            .and_then(|account| account.benefits.as_ref())
            .ok_or_else(|| {
                "the login response carried no benefits package to price the upload from".to_owned()
            })?;
        i64::try_from(benefits.texture_upload_cost_for(width, height).0).map_err(|_overflow| {
            "the benefits texture upload cost does not fit an L$ amount".to_owned()
        })
    }
}

/// The folder a viewer would upload a `folder_type` asset into: the system
/// folder of that type, or the inventory root when the grid named none.
///
/// Read out of the session's held folder model, which the login skeleton seeds
/// with every folder's preferred type before any contents fetch — the same
/// route `current-outfit-folder` takes to find the COF, and the only reliable
/// one on modern Second Life, where a descendents reply does not echo
/// `type_default`.
///
/// # Errors
///
/// Propagates the query's own timeout.
async fn destination_folder(
    session: &mut Session,
    folder_type: FolderType,
) -> Result<Option<InventoryFolderKey>, TestFailure> {
    session.send(Command::QueryInventoryFolders).await?;
    let folders: Vec<FolderInfo> = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryFolders(folders) => Some(folders.to_vec()),
            _other => None,
        })
        .await?;
    if let Some(folder) = folders
        .iter()
        .find(|folder| folder.folder_type == folder_type)
    {
        return Ok(Some(folder.folder_id));
    }

    session.send(Command::QueryInventoryRoots).await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::InventoryRoots { agent_root, .. } => Some(*agent_root),
            _other => None,
        })
        .await
}

/// The agent's current L$ balance, from a fresh `MoneyBalanceRequest`.
///
/// # Errors
///
/// Propagates the reply's own timeout — a grid with no money module answers
/// nothing, and a case that needs a balance cannot proceed without one.
async fn current_balance(session: &mut Session) -> Result<i64, TestFailure> {
    session.send(Command::RequestMoneyBalance).await?;
    let balance: MoneyBalance = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::MoneyBalance(balance) => Some(balance.clone()),
            _other => None,
        })
        .await?;
    Ok(i64::try_from(balance.balance.0).unwrap_or(i64::MAX))
}

/// Wraps `text` in the Second Life notecard asset format (`Linden text version
/// 2`) — the bytes a viewer POSTs for a notecard upload.
fn notecard_bytes(text: &str) -> Vec<u8> {
    format!(
        "Linden text version 2\n{{\nLLEmbeddedItems version 1\n{{\ncount 0\n}}\nText length {}\n{}}}\n",
        text.len(),
        text,
    )
    .into_bytes()
}
