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

use sl_proto::{EconomyData, LandImpact};
use sl_types::key::AgentKey;
use sl_types::money::LindenAmount;
use sl_wire::{
    BuyCurrencyRequest, CurrencyQuote, CurrencyQuoteRequest, HelperFailure, HelperOutcome,
    LandPrep, LandPrepRequest, LandUseRequirement, MembershipLevel, MembershipRequirement,
};

/// The economy helper's behaviour, and the price list the simulator quotes.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyConfig {
    /// The currency symbol advertised in the login response and
    /// `SimulatorFeatures` (`currency`), e.g. `"L$"`.
    pub currency_symbol: String,
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
    /// over UDP to an `EconomyDataRequest` (see [`stock_prices`]).
    pub prices: EconomyData,
}

impl Default for EconomyConfig {
    /// A working site: L$ priced at US$ 2.50 per 1000, no upgrades
    /// required, the fixed token `fake-grid-confirm`.
    fn default() -> Self {
        Self {
            currency_symbol: "L$".to_owned(),
            us_cents_per_thousand_linden: 250,
            site_valid: true,
            membership_upgrade: false,
            land_use_upgrade: false,
            confirm_token: "fake-grid-confirm".to_owned(),
            prices: stock_prices(),
        }
    }
}

/// The stock price list an `EconomyDataRequest` is answered with.
///
/// Every L$ amount is **distinct**, and deliberately not a round table of
/// zeroes the way a stock OpenSim region answers (its `SampleMoneyModule`
/// defaults every price to `0` and group creation to `-1`): a reply whose
/// fields are all the same number cannot tell a test that the encoder wrote a
/// price into the wrong slot. The two Land Impact figures are a full region's
/// budget and a plausible part of it, so the "capacity is positive and usage
/// fits inside it" check a viewer makes has something to check.
#[must_use]
pub const fn stock_prices() -> EconomyData {
    EconomyData {
        object_capacity: LandImpact(15_000),
        object_count: LandImpact(250),
        price_energy_unit: LindenAmount(1),
        price_object_claim: LindenAmount(2),
        price_public_object_decay: LindenAmount(3),
        price_public_object_delete: LindenAmount(4),
        price_parcel_claim: LindenAmount(5),
        price_parcel_claim_factor: 1.0,
        price_upload: LindenAmount(10),
        price_rent_light: LindenAmount(6),
        teleport_min_price: LindenAmount(7),
        teleport_price_exponent: 2.0,
        energy_efficiency: 1.0,
        price_object_rent: 8.0,
        price_object_scale_factor: 10.0,
        price_parcel_rent: LindenAmount(9),
        price_group_create: LindenAmount(100),
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
    use pretty_assertions::assert_eq;
    use sl_wire::{BuyCurrencyRequest, CurrencyQuoteRequest, HelperOutcome, ViewerVersionInfo};
    use uuid::Uuid;

    use super::{EconomyConfig, EconomyEvent, stock_prices};

    fn quote_request(amount: i32) -> CurrencyQuoteRequest {
        CurrencyQuoteRequest {
            agent_id: Uuid::from_u128(7),
            secure_session_id: Uuid::from_u128(8),
            language: "en".to_owned(),
            currency_buy: amount,
            viewer: ViewerVersionInfo::default(),
        }
    }

    /// Every L$ price the stock list quotes is a different number, and the
    /// region's usage fits inside its budget.
    ///
    /// The first half is what makes the `economy-data` reply diagnostic: with
    /// seventeen fields on one message, a table of equal amounts cannot tell a
    /// test that the encoder wrote a price into the wrong slot. The second is
    /// the coherence a viewer checks before believing the capacity at all.
    #[test]
    fn every_stock_price_is_distinct_and_the_capacity_is_coherent() {
        let prices = stock_prices();
        let mut amounts = vec![
            prices.price_energy_unit.0,
            prices.price_object_claim.0,
            prices.price_public_object_decay.0,
            prices.price_public_object_delete.0,
            prices.price_parcel_claim.0,
            prices.price_upload.0,
            prices.price_rent_light.0,
            prices.teleport_min_price.0,
            prices.price_parcel_rent.0,
            prices.price_group_create.0,
        ];
        let quoted = amounts.len();
        amounts.sort_unstable();
        amounts.dedup();
        assert_eq!(amounts.len(), quoted, "two stock prices quote the same L$");
        assert!(prices.object_capacity.0 > 0, "the region has no budget");
        assert!(
            prices.object_count <= prices.object_capacity,
            "the region reports more objects than it can hold"
        );
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
