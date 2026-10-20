//! The fake grid's central bake service: what a Second-Life-flavoured grid
//! bakes an agent's outfit into (`book/src/gridspec/appearance.md` § Baking).
//!
//! Second Life composites the worn layers itself, names each baked slot by an
//! id it states as that slot's `hash_id`, states the appearance — the baked
//! ids on their faces, the visual-parameter bytes — to every viewer, and
//! answers the bake trigger with the same data
//! ([`BakedOutfit`](sl_proto::BakedOutfit)). This module does the same with
//! the client's own baking code (`sl-bake`, `sl-avatar`):
//!
//! 1. **Name the bakes** when the agent arrives or asks for a bake
//!    ([`BakeService::bake`]): resolve the worn wearables out of the grid's
//!    asset store, encode their visual parameters, and id each baked slot by a
//!    hash of what it is made of. A slot no layer texture feeds is the default
//!    placeholder (`IMG_DEFAULT_AVATAR`), as Second Life's skirt was for an
//!    avatar wearing no skirt.
//! 2. **Composite on demand** ([`BakeService::render`]): a bake's pixels are
//!    composited and encoded only when the appearance service is first asked
//!    for its id. Naming costs a parse; compositing and encoding eleven 512²
//!    regions does not belong in every test that logs an agent in.
//!
//! The id is this grid's own hash of the bake's inputs, not Second Life's: the
//! census found Second Life's `hash_id` to be neither the reference viewer's
//! wearables hash nor any plain MD5 of the slot's layers (2026-10-20). What it
//! holds is the relation — a bake's id is its `hash_id`, and an unfed slot is
//! the default — and that an id changes exactly when what the bake is made of
//! does.
//!
//! The character assets (`avatar_lad.xml`, `avatar_skeleton.xml`, the static
//! layer TGAs) are read once per directory per process: thousands of tests
//! start a grid, and none of them should pay for parsing them again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use sl_avatar::{
    AVATAR_DEPTH, AVATAR_WIDTH, ResolvedParams, SkeletalDeformations, Skeleton, VisualParams,
    WearableAsset,
};
use sl_bake::{BakeRegion, StaticLayers, assemble_layers, bake_region, region_texture_ids};
use sl_proto::{
    AssetKey, AssetSource as _, BakedItem, BakedOutfit, BakedSlot, DiscardLevel, InventoryKey,
    SimSession, avatar_texture,
};
use uuid::Uuid;

use crate::assets::GridAssets;

/// The height a bake states when the skeleton cannot measure the shape: the
/// reference's rest body size.
const REST_HEIGHT: f32 = 1.707;

/// What every fake bake id is salted with, so it can never be mistaken for an
/// id some other hash produced.
const BAKE_ID_SALT: &[u8] = b"sl-fake-grid central bake";

/// The vendored character directory this crate was built beside.
#[must_use]
pub(crate) fn default_character_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../viewer-assets/character")
}

/// The parsed character assets a bake needs.
#[derive(Debug)]
struct Character {
    /// The visual-parameter table.
    params: VisualParams,
    /// The avatar skeleton, for the height.
    skeleton: Skeleton,
    /// The static layer TGAs.
    statics: StaticLayers,
}

/// The character assets read out of `dir`, once per directory per process, or
/// `None` when they cannot be read (logged once).
fn character(dir: &Path) -> Option<Arc<Character>> {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, Option<Arc<Character>>>>> = OnceLock::new();
    let mut loaded = LOADED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    loaded
        .entry(dir.to_path_buf())
        .or_insert_with(|| match load_character(dir) {
            Ok(character) => {
                for (file, error) in &character.statics.failures {
                    tracing::warn!("bake service: static layer `{file}`: {error}");
                }
                Some(Arc::new(character))
            }
            Err(error) => {
                tracing::warn!(
                    "bake service: no character assets in {}: {error}; bakes are the fixture's \
                     solid placeholders",
                    dir.display()
                );
                None
            }
        })
        .clone()
}

/// Reads and parses the character assets in `dir`.
fn load_character(dir: &Path) -> Result<Character, String> {
    let lad = fs_err::read_to_string(dir.join("avatar_lad.xml"))
        .map_err(|error| format!("avatar_lad.xml: {error}"))?;
    let params =
        VisualParams::from_xml(&lad).map_err(|error| format!("avatar_lad.xml: {error}"))?;
    let skeleton_xml = fs_err::read_to_string(dir.join("avatar_skeleton.xml"))
        .map_err(|error| format!("avatar_skeleton.xml: {error}"))?;
    let skeleton = Skeleton::from_xml(&skeleton_xml)
        .map_err(|error| format!("avatar_skeleton.xml: {error}"))?;
    Ok(Character {
        params,
        skeleton,
        statics: sl_bake::load_static_layers(dir),
    })
}

/// How one named bake is composited when it is first fetched.
#[derive(Debug, Clone)]
struct Recipe {
    /// The region baked.
    region: BakeRegion,
    /// The worn wearables it is made from.
    assets: Arc<Vec<WearableAsset>>,
}

/// The grid's bake service: shared by every session of one grid.
#[derive(Debug, Clone, Default)]
pub(crate) struct BakeService {
    /// The character assets, when they could be read.
    character: Option<Arc<Character>>,
    /// The bakes named so far and not yet composited, by id.
    recipes: Arc<Mutex<HashMap<Uuid, Recipe>>>,
}

impl BakeService {
    /// A bake service reading its character assets from `dir`.
    #[must_use]
    pub(crate) fn new(dir: &Path) -> Self {
        Self {
            character: character(dir),
            recipes: Arc::default(),
        }
    }

    /// Bakes `sim`'s agent's outfit — what it wears, resolved out of `assets`
    /// — naming each slot's bake, or `None` when there are no character assets
    /// or the agent wears nothing the store holds.
    #[must_use]
    pub(crate) fn bake(&self, sim: &SimSession, assets: &GridAssets) -> Option<BakedOutfit> {
        let character = self.character.as_ref()?;
        let agent = sim.agent_id()?.uuid();
        let (_serial, worn) = sim.agent_wearables();
        let parsed: Vec<(WearableAsset, String, Uuid)> = worn
            .iter()
            .filter_map(|wearable| {
                let asset = wearable.asset_id?;
                let bytes = assets.read().get(AssetKey::from(asset))?.to_vec();
                let text = String::from_utf8(bytes).ok()?;
                let parsed = WearableAsset::parse(&text).ok()?;
                Some((parsed, text, wearable.item_id.uuid()))
            })
            .collect();
        if parsed.is_empty() {
            return None;
        }
        let wearable_assets: Arc<Vec<WearableAsset>> = Arc::new(
            parsed
                .iter()
                .map(|(asset, _text, _item)| asset.clone())
                .collect(),
        );
        let params = &character.params;
        let visual_params = params.encode_appearance(|id| {
            wearable_assets
                .iter()
                .find_map(|asset| asset.params.get(&id).copied())
        });
        let resolved = ResolvedParams::from_appearance(params, &visual_params);
        let deform = SkeletalDeformations::from_resolved(params, &resolved);
        let height = character
            .skeleton
            .body_size(&deform)
            .map_or(REST_HEIGHT, |metrics| metrics.body_size_z);

        let mut textures = vec![Uuid::nil(); avatar_texture::COUNT];
        let mut slots = Vec::new();
        let mut recipes = self.recipes.lock().unwrap_or_else(PoisonError::into_inner);
        for region in BakeRegion::ALL {
            let texture_ids = region_texture_ids(region, &wearable_assets);
            let hash_id = if texture_ids.is_empty() {
                avatar_texture::IMG_DEFAULT_AVATAR
            } else {
                let id = bake_id(region, &texture_ids, &visual_params);
                let _previous = recipes.insert(
                    id,
                    Recipe {
                        region,
                        assets: Arc::clone(&wearable_assets),
                    },
                );
                id
            };
            if let Some(face) = textures.get_mut(region.slot()) {
                *face = hash_id;
            }
            slots.push(BakedSlot {
                name: region.name(),
                hash_id,
                texture_ids,
            });
        }
        drop(recipes);

        let inventory = sim.agent_inventory();
        let worn_item = |item: InventoryKey| {
            parsed
                .iter()
                .find(|(_asset, _text, id)| *id == item.uuid())
                .map(|(asset, text, _id)| (asset.wearable_type, text.clone()))
        };
        let wearables: Vec<BakedItem> = worn
            .iter()
            .filter_map(|wearable| {
                Some(BakedItem {
                    item: inventory.item(wearable.item_id)?.clone(),
                    agent,
                    wearable: worn_item(wearable.item_id),
                })
            })
            .collect();
        let mut cof_raw: Vec<BakedItem> = Vec::new();
        for link in inventory.current_outfit_items() {
            let target = InventoryKey::from(link.asset_id);
            cof_raw.push(BakedItem {
                item: link,
                agent,
                wearable: None,
            });
            if let Some(item) = inventory.item(target) {
                cof_raw.push(BakedItem {
                    item: item.clone(),
                    agent,
                    wearable: worn_item(target),
                });
            }
        }
        Some(BakedOutfit {
            textures,
            visual_params,
            scale: [AVATAR_DEPTH, AVATAR_WIDTH, height],
            slots,
            cof_raw,
            wearables,
        })
    }

    /// Composites and encodes the bake `id` names, files it in `assets` and
    /// returns its codestream — or `None` when this service named no such
    /// bake, or it could not be encoded (logged).
    #[must_use]
    pub(crate) fn render(&self, id: Uuid, assets: &GridAssets) -> Option<Vec<u8>> {
        let character = self.character.as_ref()?;
        let recipe = self
            .recipes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()?;
        let decode = |texture: Uuid| {
            let bytes = assets.read().get(AssetKey::from(texture))?.to_vec();
            sl_texture::decode_j2c(&bytes, DiscardLevel::FULL).ok()
        };
        let layers = assemble_layers(&recipe.assets, Some(&character.params), decode, |file| {
            character.statics.images.get(file).cloned()
        });
        let image = bake_region(
            recipe.region,
            layers.get(&recipe.region).map_or(&[], Vec::as_slice),
        )?;
        // No clothing morph masks are computed here, as in the viewer's own
        // publish: an all-255 mask is what the reference starts from.
        let mask = vec![u8::MAX; image.pixels.len().checked_div(4).unwrap_or(0)];
        match sl_texture::encode_baked_avatar_j2c(&image, &mask) {
            Ok(codestream) => {
                let _previous = assets
                    .write()
                    .insert(AssetKey::from(id), codestream.clone());
                Some(codestream)
            }
            Err(error) => {
                tracing::warn!(
                    "bake service: encoding the {} bake failed: {error}",
                    recipe.region.name()
                );
                None
            }
        }
    }
}

/// This grid's id for the bake of `region` from `texture_ids` under
/// `visual_params`: an MD5 of the salt, the region, the layers and the
/// parameters — so it changes exactly when what the bake is made of does.
fn bake_id(region: BakeRegion, texture_ids: &[Uuid], visual_params: &[u8]) -> Uuid {
    let mut context = md5::Context::new();
    context.consume(BAKE_ID_SALT);
    context.consume(region.name().as_bytes());
    for id in texture_ids {
        context.consume(id.as_bytes());
    }
    context.consume(visual_params);
    Uuid::from_bytes(context.compute().0)
}

#[cfg(test)]
mod tests {
    use super::{BakeService, bake_id, default_character_dir};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_bake::BakeRegion;
    use uuid::Uuid;

    /// A bake's id follows its inputs: the same layers and parameters, the same
    /// id; another layer, region or parameter, another.
    #[test]
    fn a_bake_id_changes_exactly_when_its_inputs_do() {
        let layer = Uuid::from_u128(1);
        let base = bake_id(BakeRegion::Head, &[layer], &[128]);
        assert_eq!(base, bake_id(BakeRegion::Head, &[layer], &[128]));
        assert_ne!(base, bake_id(BakeRegion::UpperBody, &[layer], &[128]));
        assert_ne!(
            base,
            bake_id(BakeRegion::Head, &[Uuid::from_u128(2)], &[128])
        );
        assert_ne!(base, bake_id(BakeRegion::Head, &[layer], &[127]));
    }

    /// The vendored character directory loads, so the fake grid bakes in this
    /// workspace rather than falling back to placeholders.
    #[test]
    fn the_vendored_character_assets_load() {
        let service = BakeService::new(&default_character_dir());
        assert!(
            service.character.is_some(),
            "no character assets in {}",
            default_character_dir().display()
        );
    }
}
