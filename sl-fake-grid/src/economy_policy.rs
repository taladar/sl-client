//! The economy helper policy: how the fake grid answers the buy-L$ and
//! buy-land helper calls (`currency.php`, `landtool.php`).
//!
//! Pure functions over an [`EconomyConfig`] — the HTTP glue in
//! `economy_endpoint` only parses, calls, and serialises. Nothing here moves
//! a balance: the fake grid has no money ledger, so a purchase is observable
//! only as an [`EconomyEvent`] on [`FakeGrid::economy_events`](crate::FakeGrid::economy_events).

use sl_types::key::AgentKey;
use sl_wire::{
    BuyCurrencyRequest, CurrencyQuote, CurrencyQuoteRequest, HelperFailure, HelperOutcome,
    LandPrep, LandPrepRequest, LandUseRequirement, MembershipLevel, MembershipRequirement,
};

/// The economy helper's behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        }
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

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_wire::{BuyCurrencyRequest, CurrencyQuoteRequest, HelperOutcome, ViewerVersionInfo};
    use uuid::Uuid;

    use super::{EconomyConfig, EconomyEvent};

    fn quote_request(amount: i32) -> CurrencyQuoteRequest {
        CurrencyQuoteRequest {
            agent_id: Uuid::from_u128(7),
            secure_session_id: Uuid::from_u128(8),
            language: "en".to_owned(),
            currency_buy: amount,
            viewer: ViewerVersionInfo::default(),
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
