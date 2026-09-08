//! The economy helper policy: how the fake grid answers the buy-L$ and
//! buy-land helper calls (`currency.php`, `landtool.php`).
//!
//! Pure functions over an [`EconomyConfig`] — the HTTP glue in
//! `economy_endpoint` only parses, calls, and serialises. Nothing here moves
//! a balance: the fake grid has no money ledger, so a purchase is observable
//! only as an [`EconomyEvent`] on [`FakeGrid::economy_events`](crate::FakeGrid::economy_events).
//!
//! The config also carries the **UDP** side of the same policy: the price list
//! and region object budget an `EconomyDataRequest` is answered with
//! ([`EconomyConfig::prices`]). One config, because a grid whose web helper
//! quoted one L$ rate while its simulator quoted another would be a grid no
//! viewer could reconcile.
//!
//! # The price list is the grid's, not this crate's
//!
//! Both flavours once answered one synthetic table of made-up amounts. They no
//! longer do: [`second_life_prices`] and [`open_sim_prices`] are the two real
//! lists, and [`EconomyConfig::for_grid`] picks between them, so a grid that
//! says it is Second Life quotes Second Life's prices.
//!
//! | field | Second Life | stock OpenSim |
//! | --- | --- | --- |
//! | `object_capacity` | 20 000 LI | 15 000 LI |
//! | `object_count` | 0 | 0 |
//! | `price_energy_unit` | 100 | 0 |
//! | `price_object_claim` | 10 | 0 |
//! | `price_public_object_decay` | 4 | 4 |
//! | `price_public_object_delete` | 4 | 0 |
//! | `price_parcel_claim` | 1 | 0 |
//! | `price_parcel_claim_factor` | 1.0 | 1.0 |
//! | `price_upload` | 10 | 0 |
//! | `price_rent_light` | 5 | 0 |
//! | `teleport_min_price` | 2 | 0 |
//! | `teleport_price_exponent` | 2.0 | 2.0 |
//! | `energy_efficiency` | 1.0 | 1.0 |
//! | `price_object_rent` | 1.0 | 0.0 |
//! | `price_object_scale_factor` | 10.0 | 10.0 |
//! | `price_parcel_rent` | 1 | 0 |
//! | `price_group_create` | 100 | none stated (`-1`) |
//!
//! **The five fields the two agree on are the residue of a copy.** OpenSim's
//! money module once shipped a near-verbatim copy of a Linden simulator's price
//! list, and it is still recognisable: its pre-2018 defaults were `100`, `10`,
//! `4`, `4`, `1`, `1.0`, `5`, `2`, `2.0`, `1`, `1.0`, `10`, `1` — thirteen of
//! fifteen identical to what aditi answered in 2026, the exceptions being the
//! upload charge (`0` against Second Life's L$ 10) and the group price. Then a
//! 2018 commit zeroed most of them "to no cost values, since that is our
//! default", and what survived is exactly the five above:
//! `PricePublicObjectDecay`, `PriceParcelClaimFactor`, `TeleportPriceExponent`,
//! `EnergyEfficiency` and `PriceObjectScaleFactor`. So writing the OpenSim
//! column as a round table of zeroes would be wrong in five places, and the
//! reason those five are the ones is history rather than policy.
//!
//! **Where each column comes from.** The Second Life column is one aditi run of
//! `sl-conformance`'s `economy-data` case (2026-09-08), which records all
//! seventeen fields for exactly this purpose. The OpenSim column is read off
//! `SampleMoneyModule.ReadConfigAndPopulate` — its field initialisers and its
//! `[Economy]` config defaults — because the *local* OpenSim grid cannot
//! measure it: `bin/OpenSim.ini` deliberately sets `PriceUpload = 7` and
//! `PriceGroupCreate = 11` so a live round trip is observable, so its
//! `economy-data` record differs from stock in exactly those two fields and
//! confirms the other fifteen.
//!
//! **One field arrives negative**, and it is the reason
//! [`EconomyData::price_group_create`] is an `Option`: a stock OpenSim region
//! sends `-1` for it, which this workspace's decoder used to reject — dropping
//! the whole reply rather than the one field it could not represent.
//!
//! Do not read that `None` as "free". `-1` is the reference viewer's own
//! "no reply yet" initialiser for every price, and it reached OpenSim's config
//! default from there rather than being chosen to mean anything; a grid that
//! charges nothing has `0`, which is what its fourteen sibling prices use — the
//! same 2018 commit that set this one to `-1` moved all of those *to* `0` while
//! saying it was setting "no cost values". Nothing spends the field either way:
//! OpenSim charges group creation from `IMoneyModule.GroupCreationCharge`,
//! hard-coded `0`, and never consults this number.

use sl_proto::{EconomyData, LandImpact};
use sl_types::key::AgentKey;
use sl_types::money::LindenAmount;
use sl_wire::{
    BuyCurrencyRequest, CurrencyQuote, CurrencyQuoteRequest, HelperFailure, HelperOutcome,
    LandPrep, LandPrepRequest, LandUseRequirement, MembershipLevel, MembershipRequirement,
};

use crate::imitates::ImitatedGrid;

/// The economy helper's behaviour, and the price list the simulator quotes.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyConfig {
    /// The currency symbol advertised in the login response and
    /// `SimulatorFeatures` (`currency`), or `None` to advertise none.
    ///
    /// `None` is not "no currency" — it is a grid that never says, which is
    /// what a stock OpenSim region is. Its login service defaults the field to
    /// the empty string and emits the key only `if (currency != String.Empty)`,
    /// and its `OpenSimExtras` block carries `currency-base-uri` but no symbol.
    /// A viewer then falls back to its own default, which for Firestorm is
    /// **`OS$`**, not `L$`.
    ///
    /// So the symbol is a *deployment's* choice on OpenSim rather than a
    /// grid-software constant — `StandaloneCommon.ini` offers `Currency = ""`
    /// under the comment "Ask co-operative viewers to use a different currency
    /// name", and Firestorm carries a whole multi-currency subsystem that
    /// re-renders its UI when a region's extras override the symbol mid-session.
    pub currency_symbol: Option<String>,
    /// The real-money price, in US cents per 1000 L$ (the stock 250 ≈ the
    /// historical L$ rate of US$ 2.50 per 1000 L$).
    pub us_cents_per_thousand_linden: u32,
    /// Whether the helper site is up. `false` answers every call with a
    /// failure, the viewer's "currency site unavailable" path.
    pub site_valid: bool,
    /// Whether buying land requires a membership upgrade (the preflight's
    /// `membership.upgrade`).
    pub membership_upgrade: bool,
    /// Whether buying land requires a land-use fee upgrade (`landUse.upgrade`).
    pub land_use_upgrade: bool,
    /// The `confirm` token quotes hand out and commits must echo.
    pub confirm_token: String,
    /// The grid-wide L$ price list and the region's object budget, answered
    /// over UDP to an `EconomyDataRequest` — [`second_life_prices`] or
    /// [`open_sim_prices`], picked by [`EconomyConfig::for_grid`].
    pub prices: EconomyData,
}

impl Default for EconomyConfig {
    /// The Second Life flavour's economy: [`ImitatedGrid`]'s own default.
    fn default() -> Self {
        Self::for_grid(ImitatedGrid::default())
    }
}

/// The price list and region budget **Second Life** answers an
/// `EconomyDataRequest` with.
///
/// Measured on aditi 2026-09-08 by `sl-conformance`'s `economy-data` case, all
/// seventeen fields. `object_capacity` is a full 256×256 region's 20 000 LI
/// budget; `object_count` is the zero a Linden simulator stubs it to, live
/// usage being reported through region stats and per-parcel data instead. See
/// the [module docs](self) for the table beside OpenSim's and for where each
/// column comes from.
#[must_use]
pub const fn second_life_prices() -> EconomyData {
    EconomyData {
        object_capacity: LandImpact(20_000),
        object_count: LandImpact(0),
        price_energy_unit: LindenAmount(100),
        price_object_claim: LindenAmount(10),
        price_public_object_decay: LindenAmount(4),
        price_public_object_delete: LindenAmount(4),
        price_parcel_claim: LindenAmount(1),
        price_parcel_claim_factor: 1.0,
        price_upload: LindenAmount(10),
        price_rent_light: LindenAmount(5),
        teleport_min_price: LindenAmount(2),
        teleport_price_exponent: 2.0,
        energy_efficiency: 1.0,
        price_object_rent: 1.0,
        price_object_scale_factor: 10.0,
        price_parcel_rent: LindenAmount(1),
        price_group_create: Some(LindenAmount(100)),
    }
}

/// The price list and region budget a **stock OpenSim** region answers an
/// `EconomyDataRequest` with.
///
/// Read off `SampleMoneyModule.ReadConfigAndPopulate` rather than measured,
/// because the local OpenSim grid overrides two of these on purpose — see the
/// [module docs](self). The five non-zero prices are the module's own defaults;
/// `object_capacity` is `RegionInfo`'s `MaxPrims` default and `object_count` is
/// hard-coded to `0` in the handler.
///
/// `price_group_create` is [`None`]: the module both initialises it and reads
/// its config key with `-1`. That is "no price stated" rather than "free" — see
/// the [module docs](self) for why the distinction is an accident of OpenSim's
/// history and not a protocol meaning worth honouring.
#[must_use]
pub const fn open_sim_prices() -> EconomyData {
    EconomyData {
        object_capacity: LandImpact(15_000),
        object_count: LandImpact(0),
        price_energy_unit: LindenAmount(0),
        price_object_claim: LindenAmount(0),
        price_public_object_decay: LindenAmount(4),
        price_public_object_delete: LindenAmount(0),
        price_parcel_claim: LindenAmount(0),
        price_parcel_claim_factor: 1.0,
        price_upload: LindenAmount(0),
        price_rent_light: LindenAmount(0),
        teleport_min_price: LindenAmount(0),
        teleport_price_exponent: 2.0,
        energy_efficiency: 1.0,
        price_object_rent: 0.0,
        price_object_scale_factor: 10.0,
        price_parcel_rent: LindenAmount(0),
        price_group_create: None,
    }
}

/// A purchase the helper accepted, published on
/// [`FakeGrid::economy_events`](crate::FakeGrid::economy_events).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconomyEvent {
    /// `buyCurrency` succeeded.
    CurrencyBought {
        /// The buying agent.
        agent_id: AgentKey,
        /// The L$ amount bought.
        amount: i32,
    },
    /// `buyLandPrep` succeeded (the land itself is bought through the
    /// simulator's `ParcelBuy`; this is only the web-site half).
    LandPrepared {
        /// The buying agent.
        agent_id: AgentKey,
        /// The parcel's billable area.
        billable_area: i32,
        /// The L$ also bought for the purchase.
        currency_buy: i32,
    },
}

/// The failure answered while the site is down.
fn site_down() -> HelperFailure {
    HelperFailure {
        error_message: "The fake grid's currency site is currently unavailable.".to_owned(),
        error_uri: String::new(),
    }
}

/// The failure answered for a commit whose `confirm` does not match.
fn bad_confirm() -> HelperFailure {
    HelperFailure {
        error_message: "The purchase confirmation token did not match the quote.".to_owned(),
        error_uri: String::new(),
    }
}

impl EconomyConfig {
    /// The economy `grid` runs: a working helper site with L$ priced at
    /// US$ 2.50 per 1000, no upgrades required, the fixed token
    /// `fake-grid-confirm`, and **that grid's** price list.
    ///
    /// The price list and the currency symbol follow the flavour; the rest is
    /// helper policy a test sets deliberately (is the site up, does buying land
    /// demand an upgrade, what token do quotes hand out).
    #[must_use]
    pub fn for_grid(grid: ImitatedGrid) -> Self {
        Self {
            currency_symbol: grid.currency_symbol().map(str::to_owned),
            us_cents_per_thousand_linden: 250,
            site_valid: true,
            membership_upgrade: false,
            land_use_upgrade: false,
            confirm_token: "fake-grid-confirm".to_owned(),
            prices: grid.prices(),
        }
    }

    /// The US-cent cost of `amount` L$ (rounded up), or `None` on overflow.
    #[must_use]
    pub fn cost_in_cents(&self, amount: i32) -> Option<i32> {
        let amount = u64::try_from(amount).ok()?;
        let cents = amount
            .checked_mul(u64::from(self.us_cents_per_thousand_linden))?
            .div_ceil(1000);
        i32::try_from(cents).ok()
    }

    /// Renders a cent amount the way the newer helper servers do
    /// (`estimatedLocalCost`), e.g. `US$ 2.50`.
    #[must_use]
    pub fn local_cost(cents: i32) -> String {
        format!("US$ {}.{:02}", cents / 100, cents % 100)
    }

    /// Answers `getCurrencyQuote`.
    #[must_use]
    pub fn quote(&self, request: &CurrencyQuoteRequest) -> HelperOutcome<CurrencyQuote> {
        if !self.site_valid {
            return HelperOutcome::Failed(site_down());
        }
        let Some(cents) = self.cost_in_cents(request.currency_buy) else {
            return HelperOutcome::Failed(HelperFailure {
                error_message: "That amount cannot be quoted.".to_owned(),
                error_uri: String::new(),
            });
        };
        HelperOutcome::Ok(CurrencyQuote {
            currency_buy: request.currency_buy,
            estimated_cost: Some(cents),
            estimated_local_cost: Some(Self::local_cost(cents)),
            confirm: self.confirm_token.clone(),
        })
    }

    /// Answers `buyCurrency`: the event to publish on success.
    #[must_use]
    pub fn buy_currency(&self, request: &BuyCurrencyRequest) -> HelperOutcome<EconomyEvent> {
        if !self.site_valid {
            return HelperOutcome::Failed(site_down());
        }
        if request.confirm != self.confirm_token {
            return HelperOutcome::Failed(bad_confirm());
        }
        HelperOutcome::Ok(EconomyEvent::CurrencyBought {
            agent_id: AgentKey::from(request.agent_id),
            amount: request.currency_buy,
        })
    }

    /// Answers `preflightBuyLandPrep`.
    #[must_use]
    pub fn preflight_land(&self, request: &LandPrepRequest) -> HelperOutcome<LandPrep> {
        if !self.site_valid {
            return HelperOutcome::Failed(site_down());
        }
        let cents = self.cost_in_cents(request.currency_buy);
        HelperOutcome::Ok(LandPrep {
            membership: MembershipRequirement {
                upgrade: self.membership_upgrade,
                action: if self.membership_upgrade {
                    "Upgrade to a premium membership".to_owned()
                } else {
                    String::new()
                },
                levels: if self.membership_upgrade {
                    vec![MembershipLevel {
                        id: "premium".to_owned(),
                        description: "Premium membership".to_owned(),
                    }]
                } else {
                    Vec::new()
                },
            },
            land_use: LandUseRequirement {
                upgrade: self.land_use_upgrade,
                action: if self.land_use_upgrade {
                    "Increase your land-use fee tier".to_owned()
                } else {
                    String::new()
                },
            },
            estimated_cost: cents,
            estimated_local_cost: cents.map(Self::local_cost),
            confirm: self.confirm_token.clone(),
        })
    }

    /// Answers `buyLandPrep`: the event to publish on success.
    #[must_use]
    pub fn buy_land(&self, request: &LandPrepRequest) -> HelperOutcome<EconomyEvent> {
        if !self.site_valid {
            return HelperOutcome::Failed(site_down());
        }
        if request.confirm.as_deref() != Some(self.confirm_token.as_str()) {
            return HelperOutcome::Failed(bad_confirm());
        }
        HelperOutcome::Ok(EconomyEvent::LandPrepared {
            agent_id: AgentKey::from(request.agent_id),
            billable_area: request.billable_area,
            currency_buy: request.currency_buy,
        })
    }
}

/// Answers one drained [`ServerEvent`] from the economy policy: an
/// `EconomyDataRequest` gets the grid's price list and this region's object
/// budget. Everything else is left alone.
///
/// A live simulator answers this from the money module the grid runs; the fake
/// grid answers it from the same [`EconomyConfig`] its web helper quotes from.
pub(crate) fn answer_economy_request(
    prices: &EconomyData,
    sim: &mut sl_proto::SimSession,
    event: &sl_proto::ServerEvent,
    now: std::time::Instant,
) {
    if matches!(event, sl_proto::ServerEvent::RequestEconomyData)
        && let Err(error) = sim.send_economy_data(prices, now)
    {
        tracing::warn!("answering an economy data request failed: {error}");
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_wire::{BuyCurrencyRequest, CurrencyQuoteRequest, HelperOutcome, ViewerVersionInfo};
    use uuid::Uuid;

    use sl_types::money::LindenAmount;

    use super::{EconomyConfig, EconomyEvent, ImitatedGrid, open_sim_prices, second_life_prices};

    fn quote_request(amount: i32) -> CurrencyQuoteRequest {
        CurrencyQuoteRequest {
            agent_id: Uuid::from_u128(7),
            secure_session_id: Uuid::from_u128(8),
            language: "en".to_owned(),
            currency_buy: amount,
            viewer: ViewerVersionInfo::default(),
        }
    }

    /// Each flavour quotes its own grid's prices, and a grid built with no
    /// economy at all gets Second Life's.
    ///
    /// This is the check that the two tables are not the same table wearing two
    /// names. It pins the two numbers a viewer actually spends against — the
    /// upload charge and the group-creation fee — because those are the pair
    /// where a wrong answer costs a test avatar real L$ rather than a wrong
    /// label.
    #[test]
    fn each_flavour_quotes_its_own_grids_prices() {
        assert_eq!(ImitatedGrid::SecondLife.prices(), second_life_prices());
        assert_eq!(ImitatedGrid::OpenSim.prices(), open_sim_prices());
        assert_ne!(second_life_prices(), open_sim_prices());
        assert_eq!(EconomyConfig::default().prices, second_life_prices());

        assert_eq!(second_life_prices().price_upload, LindenAmount(10));
        assert_eq!(open_sim_prices().price_upload, LindenAmount(0));
        assert_eq!(
            second_life_prices().price_group_create,
            Some(LindenAmount(100))
        );
        assert_eq!(open_sim_prices().price_group_create, None);
    }

    /// Both grids report a positive region budget with the usage inside it, and
    /// both stub the usage to zero.
    ///
    /// That coherence is what a viewer checks before believing the capacity at
    /// all, and `economy-data` asserts it on every grid — so a price list that
    /// failed it here would fail the live cases too. The zero is not this
    /// crate's simplification: a Linden simulator sends it (aditi, measured)
    /// and OpenSim's handler hard-codes it, reporting live usage through region
    /// stats and per-parcel data instead.
    #[test]
    fn both_grids_report_a_coherent_and_stubbed_region_budget() {
        for prices in [second_life_prices(), open_sim_prices()] {
            assert!(prices.object_capacity.0 > 0, "the region has no budget");
            assert!(
                prices.object_count <= prices.object_capacity,
                "the region reports more objects than it can hold"
            );
            assert_eq!(prices.object_count, sl_proto::LandImpact(0));
        }
    }

    /// The five fields the two grids agree on stay agreed.
    ///
    /// They are the ones OpenSim's `SampleMoneyModule` gives a non-zero
    /// default, each copied from what a Linden simulator was sending when it
    /// was written — so writing the OpenSim column as a round table of zeroes
    /// would be wrong in five places, and this is what says so out loud.
    #[test]
    fn the_two_grids_still_agree_where_open_sim_copied_a_linden_default() {
        let sl = second_life_prices();
        let opensim = open_sim_prices();
        assert_eq!(
            sl.price_public_object_decay,
            opensim.price_public_object_decay
        );
        // The four scalars compare by bit pattern rather than by value: both
        // sides are the same literal written twice, so an exact match is the
        // claim, and `to_bits` makes it without asking for a float tolerance
        // that would only hide a genuinely different constant.
        for (field, lhs, rhs) in [
            (
                "price_parcel_claim_factor",
                sl.price_parcel_claim_factor,
                opensim.price_parcel_claim_factor,
            ),
            (
                "teleport_price_exponent",
                sl.teleport_price_exponent,
                opensim.teleport_price_exponent,
            ),
            (
                "energy_efficiency",
                sl.energy_efficiency,
                opensim.energy_efficiency,
            ),
            (
                "price_object_scale_factor",
                sl.price_object_scale_factor,
                opensim.price_object_scale_factor,
            ),
        ] {
            assert_eq!(lhs.to_bits(), rhs.to_bits(), "{field} diverged");
        }
    }

    #[test]
    fn quotes_price_and_round_up() -> Result<(), String> {
        let config = EconomyConfig::default();
        assert_eq!(config.cost_in_cents(1000), Some(250));
        assert_eq!(config.cost_in_cents(1), Some(1));
        assert_eq!(config.cost_in_cents(0), Some(0));
        assert_eq!(config.cost_in_cents(-5), None);
        assert_eq!(EconomyConfig::local_cost(250), "US$ 2.50");
        assert_eq!(EconomyConfig::local_cost(7), "US$ 0.07");
        let HelperOutcome::Ok(quote) = config.quote(&quote_request(1000)) else {
            return Err("expected a quote".to_owned());
        };
        assert_eq!(quote.estimated_cost, Some(250));
        assert_eq!(quote.estimated_local_cost.as_deref(), Some("US$ 2.50"));
        assert_eq!(quote.confirm, "fake-grid-confirm");
        Ok(())
    }

    #[test]
    fn buy_needs_the_token_and_a_live_site() {
        let config = EconomyConfig::default();
        let mut buy = BuyCurrencyRequest {
            agent_id: Uuid::from_u128(7),
            secure_session_id: Uuid::from_u128(8),
            language: "en".to_owned(),
            currency_buy: 500,
            confirm: "wrong".to_owned(),
            estimated_cost: None,
            estimated_local_cost: None,
            password: None,
            viewer: ViewerVersionInfo::default(),
        };
        assert!(matches!(
            config.buy_currency(&buy),
            HelperOutcome::Failed(_)
        ));
        buy.confirm = config.confirm_token.clone();
        assert_eq!(
            config.buy_currency(&buy),
            HelperOutcome::Ok(EconomyEvent::CurrencyBought {
                agent_id: Uuid::from_u128(7).into(),
                amount: 500,
            })
        );
        let down = EconomyConfig {
            site_valid: false,
            ..EconomyConfig::default()
        };
        assert!(matches!(
            down.quote(&quote_request(1)),
            HelperOutcome::Failed(_)
        ));
        assert!(matches!(down.buy_currency(&buy), HelperOutcome::Failed(_)));
    }
}
