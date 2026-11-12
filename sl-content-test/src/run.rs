//! Playing a case: a fake grid built from it, ticked by hand until its avatars
//! have done what they were told and the region has settled, and everything
//! observable collected.
//!
//! The grid is in-process and deterministic: its region's heartbeat is stepped
//! by the runner, every id and random draw comes from the case's seed, and a
//! scripted avatar's steps wait on ticks, not on time. What is collected is
//! what a person standing in the region could find out, and one thing nobody
//! there could: the line a script stopped at.
//!
//! - every line said in the region, on every channel, wherever
//!   ([`FakeGrid::overhear`]);
//! - every script's run-time error, with its line ([`FakeGrid::script_faults`]);
//! - every call to a library function the grid only stubs
//!   ([`FakeGrid::stubbed_calls`]);
//! - each avatar's transcript — what it was asked, shown, offered, and did —
//!   and every line it was told, owner-says and lines said to it alone
//!   included;
//! - each script compiled as the imitated grid compiles it, before the run;
//! - where every prim stands at the end;
//! - every key a script names that the case did not supply.

use std::collections::BTreeMap;
use std::time::Duration;

use sl_fake_grid::chat::Line;
use sl_fake_grid::{
    AccountConfig, FakeGrid, FakeGridBuilder, HeardLine, HeartbeatConfig, ImitatedGrid, Progress,
    RegionConfig, RegionFixture, ScriptFault, ScriptedAvatar, StubbedCall,
};
use sl_proto::{
    AssetKey, AssetType, ChatType, InventoryType, RegionLocalObjectId, ScriptTarget,
    TaskInventoryItem,
};
use sl_types::key::{AgentKey, InventoryKey, ObjectKey, OwnerKey};
use sl_types::lsl::Vector;

use crate::case::{AssetKind, Case, Content, Item, Prim};

/// How long one tick, or an avatar's arrival, may take in wall-clock time
/// before the run is given up as hung rather than slow.
const WALL_CLOCK_WAIT: Duration = Duration::from_secs(60);

/// How long the runner pauses between two ticks, so the avatars' clients act
/// on the last tick before the next: wall-clock time that changes nothing
/// observable, since every step waits on ticks.
const CLIENT_PAUSE: Duration = Duration::from_millis(10);

/// The first region-local id the case's prims are numbered from.
const FIRST_LOCAL_ID: u32 = 0xC000;

/// The high bits of every key the runner mints for a case.
const MINTED: u128 = 0xC0_7E57_0000_0000_0000_0000_0000_0000;

/// The password of every account the runner registers for an owner who is
/// not one of the case's avatars, which nothing logs in with.
const OWNER_PASSWORD: &str = "content-owner";

/// Why a case could not be played to the end — a fault of the run, not of
/// the content under test.
#[derive(Debug, thiserror::Error)]
pub enum PlayError {
    /// The grid did not start, or would not answer about its region.
    #[error("the fake grid failed: {0}")]
    Grid(#[from] sl_fake_grid::Error),
    /// An avatar did not arrive in the region.
    #[error("the avatar {avatar} did not arrive: {progress:?}")]
    Arrival {
        /// The avatar.
        avatar: String,
        /// Where it got to.
        progress: Progress,
    },
    /// Something the runner waited for took longer than any run should.
    #[error("the run hung: {0} took longer than {WALL_CLOCK_WAIT:?} of wall-clock time")]
    Hung(String),
}

/// A script that did not compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileFailure {
    /// The prim it is in.
    pub object: String,
    /// Its name.
    pub script: String,
    /// The errors, as the imitated grid words them.
    pub errors: Vec<String>,
}

/// A key a script names that the case does not supply: the fake grid answers
/// a fetch of it as not found, where the live grid may well have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupplied {
    /// The prim the script is in.
    pub object: String,
    /// The script.
    pub script: String,
    /// The key.
    pub key: uuid::Uuid,
}

/// What one avatar did in a run.
#[derive(Debug, Clone)]
pub struct AvatarOutcome {
    /// Its name.
    pub name: String,
    /// How far it got.
    pub progress: Progress,
    /// The index of the step it was on when the run ended, unless it had
    /// finished.
    pub step: Option<usize>,
    /// Its transcript, line by line.
    pub transcript: Vec<String>,
    /// Every line it was told.
    pub heard: Vec<HeardLine>,
}

/// Everything observable of one run of a case on one grid.
#[derive(Debug, Clone)]
pub struct Observed {
    /// The grid it ran on.
    pub grid: ImitatedGrid,
    /// How many ticks it ran.
    pub ticks: u32,
    /// Whether every avatar finished its steps within the tick budget.
    pub finished: bool,
    /// Every line said in the region, in order; typing notices left out.
    pub said: Vec<Line>,
    /// Every run-time error, in order.
    pub faults: Vec<ScriptFault>,
    /// Each stubbed function a script called, once per script and function.
    pub stubbed: Vec<StubbedCall>,
    /// What each avatar did, in the case's order.
    pub avatars: Vec<AvatarOutcome>,
    /// Where each named prim stands at the end, in region metres, in the
    /// region's order.
    pub positions: Vec<(String, Vector)>,
    /// The scripts that did not compile.
    pub compile_failures: Vec<CompileFailure>,
    /// The keys scripts name that the case does not supply.
    pub unsupplied: Vec<Unsupplied>,
}

/// Plays `case` on a fake grid imitating `grid`.
///
/// # Errors
///
/// [`PlayError`] when the grid fails or the run hangs; what the content does
/// wrong is in the [`Observed`], not an error.
pub async fn run(case: &Case, grid: ImitatedGrid) -> Result<Observed, PlayError> {
    let region_name = RegionConfig::default().name;
    let built = build(case, grid, &region_name);
    let mut builder = FakeGridBuilder::new()
        .deterministic(case.seed)
        .imitates(grid);
    for account in built.accounts {
        builder = builder.account(account);
    }
    let fake = builder.region(built.region).start().await?;
    let played = play(case, &fake, &region_name).await;
    fake.shutdown();
    let mut observed = played?;
    observed.grid = grid;
    observed.compile_failures = compile(case, grid);
    observed.unsupplied = unsupplied(case);
    Ok(observed)
}

/// Ticks the grid's region through the case and collects what it saw.
async fn play(case: &Case, fake: &FakeGrid, region: &str) -> Result<Observed, PlayError> {
    let mut faults = fake.script_faults();
    let mut stubs = fake.stubbed_calls();
    let mut said = fake.overhear(region)?;
    let handles = fake.scripted_avatars().to_vec();
    for handle in &handles {
        let progress = tokio::time::timeout(WALL_CLOCK_WAIT, handle.arrived())
            .await
            .map_err(|_elapsed| PlayError::Hung(format!("{}'s arrival", handle.name())))?;
        if !matches!(progress, Progress::Running | Progress::Finished) {
            return Err(PlayError::Arrival {
                avatar: handle.name().to_owned(),
                progress,
            });
        }
    }
    let done = || {
        handles
            .iter()
            .all(|handle| matches!(handle.progress(), Progress::Finished | Progress::Stopped(_)))
    };
    let mut ticks = 0_u32;
    let mut settled = 0_u32;
    while ticks < case.max_ticks && settled < case.settle_ticks {
        tokio::time::timeout(WALL_CLOCK_WAIT, fake.advance_one_tick(region))
            .await
            .map_err(|_elapsed| PlayError::Hung(format!("tick {}", ticks.saturating_add(1))))??;
        ticks = ticks.saturating_add(1);
        tokio::time::sleep(CLIENT_PAUSE).await;
        if done() {
            settled = settled.saturating_add(1);
        }
    }
    let finished = handles
        .iter()
        .all(|handle| handle.progress() == Progress::Finished);
    let mut lines = Vec::new();
    while let Ok(line) = said.try_recv() {
        if !matches!(line.chat_type, ChatType::StartTyping | ChatType::StopTyping) {
            lines.push(line);
        }
    }
    let mut stopped = Vec::new();
    while let Ok(fault) = faults.try_recv() {
        stopped.push(fault);
    }
    let mut stubbed = Vec::new();
    while let Ok(call) = stubs.try_recv() {
        if !stubbed.contains(&call) {
            stubbed.push(call);
        }
    }
    let positions = fake.with_region_world(region, |world| {
        world
            .objects
            .iter()
            .filter(|object| object.pcode != sl_proto::pcode::AVATAR)
            .filter_map(|object| {
                let name = world.properties_of(object.local_id)?.name;
                let root = (object.parent_id.0 != 0)
                    .then(|| world.objects.get(object.parent_id))
                    .flatten();
                let at = root.map_or_else(
                    || object.motion.position.clone(),
                    |root| {
                        let offset = rotated(&root.motion.rotation, &object.motion.position);
                        Vector {
                            x: root.motion.position.x + offset.x,
                            y: root.motion.position.y + offset.y,
                            z: root.motion.position.z + offset.z,
                        }
                    },
                );
                Some((name, at))
            })
            .collect()
    })?;
    let avatars = handles
        .iter()
        .map(|handle| AvatarOutcome {
            name: handle.name().to_owned(),
            progress: handle.progress(),
            step: handle.step(),
            transcript: handle
                .transcript()
                .iter()
                .map(ToString::to_string)
                .collect(),
            heard: handle.heard(),
        })
        .collect();
    Ok(Observed {
        grid: ImitatedGrid::SecondLife,
        ticks,
        finished,
        said: lines,
        faults: stopped,
        stubbed,
        avatars,
        positions,
        compile_failures: Vec::new(),
        unsupplied: Vec::new(),
    })
}

/// What a case builds: the accounts of owners who are not avatars, and the
/// region holding the objects and the avatars.
struct Built {
    /// The owners' accounts.
    accounts: Vec<AccountConfig>,
    /// The region.
    region: RegionConfig,
}

/// Mints the case's ids, in the order the case states its things, so two
/// runs of one case mint the same ones.
struct Mint {
    /// The next region-local id.
    next_local: u32,
    /// The next item or asset serial.
    next_serial: u128,
}

impl Mint {
    /// A fresh region-local id and the key minted with it.
    fn prim(&mut self) -> (RegionLocalObjectId, ObjectKey) {
        let local = self.next_local;
        self.next_local = self.next_local.saturating_add(1);
        (
            RegionLocalObjectId(local),
            ObjectKey::from(uuid::Uuid::from_u128(MINTED | (u128::from(local) << 32))),
        )
    }

    /// A fresh key for an item or an asset.
    const fn key(&mut self) -> uuid::Uuid {
        self.next_serial = self.next_serial.saturating_add(1);
        uuid::Uuid::from_u128(MINTED | self.next_serial)
    }
}

/// The agent id of the `index`th named person of a case (its avatars first,
/// then the owners who are not avatars).
fn agent_key(index: usize) -> AgentKey {
    let index = u128::try_from(index).unwrap_or(u128::MAX);
    AgentKey::from(uuid::Uuid::from_u128(
        MINTED | (0xA6E7 << 64) | index.saturating_add(1),
    ))
}

/// Builds `case` for a grid imitating `grid`, its one region called
/// `region_name`.
fn build(case: &Case, grid: ImitatedGrid, region_name: &str) -> Built {
    let mut people: BTreeMap<String, AgentKey> = BTreeMap::new();
    let mut avatars = Vec::new();
    for (index, avatar) in case.avatars.iter().enumerate() {
        let agent = agent_key(index);
        let _previous = people.insert(avatar.name(), agent);
        let mut scripted =
            ScriptedAvatar::new(avatar.first.clone(), avatar.last.clone()).answering(avatar.policy);
        if avatar.dressed {
            scripted = scripted.dressed();
        }
        scripted.account.agent_id = Some(agent);
        if let Some(start) = &avatar.start {
            scripted.account.start_region = Some(region_name.to_owned());
            scripted = scripted.starting_at(start.clone());
        }
        for (ticks, action) in &avatar.steps {
            scripted = scripted.after_ticks(*ticks, action.clone());
        }
        avatars.push(scripted);
    }
    let mut accounts = Vec::new();
    for object in &case.objects {
        if people.contains_key(&object.owner) {
            continue;
        }
        let agent = agent_key(people.len());
        let _previous = people.insert(object.owner.clone(), agent);
        let (first, last) = object
            .owner
            .split_once(' ')
            .unwrap_or((object.owner.as_str(), "Resident"));
        accounts.push(AccountConfig {
            agent_id: Some(agent),
            ..AccountConfig::new(first, last, OWNER_PASSWORD)
        });
    }

    let mut fixture = RegionFixture::new();
    let mut mint = Mint {
        next_local: FIRST_LOCAL_ID,
        next_serial: 0,
    };
    for (id, bytes) in &case.assets {
        let _previous = fixture.assets.insert(AssetKey::from(*id), bytes.clone());
    }
    for object in &case.objects {
        let owner = people
            .get(&object.owner)
            .copied()
            .unwrap_or_else(|| agent_key(usize::MAX));
        let (root_id, root_key) = mint.prim();
        let mut root = prim_object(
            root_id,
            root_key,
            owner,
            object.position.clone(),
            &object.root,
        );
        if let Some(click) = object.click {
            root.click_action = click.to_code();
        }
        fixture.world.objects.push(root);
        stock(
            &mut fixture,
            &mut mint,
            root_id,
            root_key,
            owner,
            &object.root,
        );
        for link in &object.links {
            let (local_id, key) = mint.prim();
            let mut child = prim_object(local_id, key, owner, link.offset.clone(), link);
            child.parent_id = root_id;
            fixture.world.objects.push(child);
            stock(&mut fixture, &mut mint, local_id, key, owner, link);
        }
    }
    let mut region = fixture.into_region(RegionConfig {
        heartbeat: HeartbeatConfig::stepped(case.step),
        name: region_name.to_owned(),
        ..RegionConfig::default()
    });
    region.scripted_avatars = avatars;
    tracing::debug!(
        case = case.name,
        ?grid,
        accounts = accounts.len(),
        "built a case"
    );
    Built { accounts, region }
}

/// A box prim at `position` (region metres for a root, root-relative for a
/// child) named and sized as `prim` says.
fn prim_object(
    local_id: RegionLocalObjectId,
    key: ObjectKey,
    owner: AgentKey,
    position: Vector,
    prim: &Prim,
) -> sl_proto::Object {
    let mut object = sl_fake_grid::box_prim(local_id, key, owner, position, prim.size.clone());
    let mut properties = sl_fake_grid::default_object_properties(&object);
    prim.name.clone_into(&mut properties.name);
    object.properties = Some(properties);
    object
}

/// Puts `prim`'s contents into the prim `local_id` (keyed `key`), and their
/// assets into the region's store.
fn stock(
    fixture: &mut RegionFixture,
    mint: &mut Mint,
    local_id: RegionLocalObjectId,
    key: ObjectKey,
    owner: AgentKey,
    prim: &Prim,
) {
    if prim.contents.is_empty() {
        return;
    }
    let mut items = Vec::new();
    let mut running = Vec::new();
    for content in &prim.contents {
        let item_id = InventoryKey::from(mint.key());
        let (asset, asset_type, inv_type, bytes) = asset_of(mint, content);
        if let Item::Script { running: true, .. } = content.item {
            running.push(item_id);
        }
        if let Some(bytes) = bytes {
            let _previous = fixture.assets.insert(asset, bytes);
        }
        items.push(TaskInventoryItem {
            item_id,
            parent_task: key,
            asset_id: Some(asset),
            asset_type,
            inv_type,
            name: content.name.clone(),
            owner: OwnerKey::Agent(owner),
            creator_id: owner,
            last_owner_id: owner,
            ..sl_fake_grid::scenario::stock_script_item()
        });
    }
    let mut contents = sl_fake_grid::TaskInventory::stated(1, items);
    contents.running.extend(running);
    if fixture
        .world
        .add_task_inventory(local_id, contents)
        .is_err()
    {
        tracing::error!(prim = prim.name, "a case's prim is not in its region");
    }
}

/// An item's asset key, types, and the bytes stored under the key (none for
/// an object item, which has no asset behind it).
fn asset_of(
    mint: &mut Mint,
    content: &Content,
) -> (AssetKey, AssetType, InventoryType, Option<Vec<u8>>) {
    match &content.item {
        Item::Script { source, .. } => (
            AssetKey::from(mint.key()),
            AssetType::ScriptText,
            InventoryType::Script,
            Some(source.as_bytes().to_vec()),
        ),
        Item::Notecard { text } => (
            AssetKey::from(mint.key()),
            AssetType::Notecard,
            InventoryType::Notecard,
            Some(
                sl_notecard::Notecard {
                    source_version: sl_notecard::NotecardVersion::V2,
                    items: Vec::new(),
                    text: text.clone(),
                }
                .encode(),
            ),
        ),
        Item::Object => (
            AssetKey::from(mint.key()),
            AssetType::Object,
            InventoryType::Object,
            None,
        ),
        Item::Asset { kind, bytes, id } => {
            let key = AssetKey::from(id.unwrap_or_else(|| mint.key()));
            let (asset_type, inv_type) = match kind {
                AssetKind::Animation => (AssetType::Animation, InventoryType::Animation),
                AssetKind::Sound => (AssetType::Sound, InventoryType::Sound),
                AssetKind::Texture => (AssetType::Texture, InventoryType::Texture),
            };
            (key, asset_type, inv_type, Some(bytes.clone()))
        }
    }
}

/// Every script of `case` that does not compile as `grid` compiles it.
fn compile(case: &Case, grid: ImitatedGrid) -> Vec<CompileFailure> {
    let compiler = grid.script_compile_policy().compiler;
    case.scripts()
        .filter_map(|(prim, content)| {
            let Item::Script { source, .. } = &content.item else {
                return None;
            };
            let outcome = sl_fake_grid::compile_source(
                compiler,
                ScriptTarget::Mono.to_wire(),
                source.as_bytes(),
                AssetKey::from(uuid::Uuid::nil()),
            );
            (!outcome.compiled).then(|| CompileFailure {
                object: prim.name.clone(),
                script: content.name.clone(),
                errors: outcome.errors,
            })
        })
        .collect()
}

/// Every key a script of `case` names that is neither supplied by the case,
/// a built-in animation, nor the null key.
fn unsupplied(case: &Case) -> Vec<Unsupplied> {
    let supplied: Vec<uuid::Uuid> = case
        .assets
        .iter()
        .map(|(id, _bytes)| *id)
        .chain(
            case.objects
                .iter()
                .flat_map(|object| std::iter::once(&object.root).chain(&object.links))
                .flat_map(|prim| &prim.contents)
                .filter_map(|content| match &content.item {
                    Item::Asset { id, .. } => *id,
                    Item::Script { .. } | Item::Notecard { .. } | Item::Object => None,
                }),
        )
        .collect();
    let mut found = Vec::new();
    for (prim, content) in case.scripts() {
        let Item::Script { source, .. } = &content.item else {
            continue;
        };
        for key in keys_in(source) {
            if key.is_nil() || supplied.contains(&key) || sl_anim::builtin_animation(key).is_some()
            {
                continue;
            }
            let unsupplied = Unsupplied {
                object: prim.name.clone(),
                script: content.name.clone(),
                key,
            };
            if !found.contains(&unsupplied) {
                found.push(unsupplied);
            }
        }
    }
    found
}

/// `v` turned by the unit quaternion `q`.
fn rotated(q: &sl_types::lsl::Rotation, v: &Vector) -> Vector {
    // v + 2s(q × v) + 2q × (q × v), with q's vector part (x, y, z).
    let cross = |ax: f32, ay: f32, az: f32, bx: f32, by: f32, bz: f32| {
        (ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx)
    };
    let (tx, ty, tz) = cross(q.x, q.y, q.z, v.x, v.y, v.z);
    let (tx, ty, tz) = (2.0 * tx, 2.0 * ty, 2.0 * tz);
    let (ux, uy, uz) = cross(q.x, q.y, q.z, tx, ty, tz);
    Vector {
        x: v.x + q.s * tx + ux,
        y: v.y + q.s * ty + uy,
        z: v.z + q.s * tz + uz,
    }
}

/// Every hyphenated uuid written in `source`.
fn keys_in(source: &str) -> Vec<uuid::Uuid> {
    source
        .split(|character: char| !(character.is_ascii_hexdigit() || character == '-'))
        .filter(|token| token.len() == 36)
        .filter_map(|token| uuid::Uuid::try_parse(token).ok())
        .collect()
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    /// A script's uuid literals are found wherever they are written, and
    /// nothing else is taken for one.
    #[test]
    fn keys_are_found_in_a_source() {
        let source = r#"default { state_entry() {
            llPlaySound("0b8fbd1f-0eae-4b3f-a5cc-a5b33ac2f9e2", 1.0);
            llStartAnimation("dance1");
            key k = "00000000-0000-0000-0000-000000000000";
            string s = "not-a-key-0b8fbd1f";
        } }"#;
        assert_eq!(
            keys_in(source),
            vec![
                uuid::uuid!("0b8fbd1f-0eae-4b3f-a5cc-a5b33ac2f9e2"),
                uuid::Uuid::nil()
            ]
        );
    }

    /// A quarter turn about Z takes +X to +Y.
    #[test]
    fn a_quarter_turn_takes_x_to_y() {
        let half = std::f32::consts::FRAC_PI_4;
        let turned = rotated(
            &sl_types::lsl::Rotation {
                x: 0.0,
                y: 0.0,
                z: half.sin(),
                s: half.cos(),
            },
            &Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        );
        assert!(
            turned.x.abs() < 1e-6 && (turned.y - 1.0).abs() < 1e-6,
            "{turned:?}"
        );
    }

    /// Two runs of one case mint the same ids, the avatars' first.
    #[test]
    fn ids_are_minted_in_order() {
        let mut mint = Mint {
            next_local: FIRST_LOCAL_ID,
            next_serial: 0,
        };
        let (first, _key) = mint.prim();
        let (second, _key) = mint.prim();
        assert_eq!((first.0, second.0), (FIRST_LOCAL_ID, FIRST_LOCAL_ID + 1));
        assert_ne!(agent_key(0), agent_key(1));
        assert_eq!(agent_key(3), agent_key(3));
    }
}
