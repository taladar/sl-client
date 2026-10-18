//! The symbol the grid's money is shown with: `L$` on Second Life, `OS$` on an
//! OpenSim grid that names none, or whatever symbol the grid names.
//!
//! The reference (Firestorm's OpenSim build) decides it in three steps: the
//! login response's `currency` (`llstartup.cpp`), else `OS$` on an OpenSim
//! grid, else `L$`; and a region's `SimulatorFeatures` `OpenSimExtras`
//! `currency` overrides that while the agent is there
//! (`lfsimfeaturehandler.cpp`, `Tea::setRegionCurrency`). Neither Second Life
//! nor a stock OpenSim sends either key (`book/src/gridspec/login.md`,
//! `book/src/gridspec/region-arrival.md`), so in practice the symbol is `L$`
//! on Second Life and `OS$` on OpenSim.
//!
//! The reference knows it is on OpenSim from the grid it logged in to; this
//! viewer knows it from the region's features, as the rest of the viewer does
//! (an `OpenSimExtras` map is sent by every OpenSim region and by no Second
//! Life one), so the symbol reads `L$` until the first region's features
//! arrive.

use bevy::prelude::*;

use sl_client_bevy::{SlEvent, SlSessionEvent};

/// The symbol a grid that names none shows Second Life's money with.
pub const LINDEN_SYMBOL: &str = "L$";

/// The symbol a grid that names none shows OpenSim's money with.
pub const OPENSIM_SYMBOL: &str = "OS$";

/// What the grid said about its currency.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct GridCurrency {
    /// The login response's `currency`.
    login: Option<String>,
    /// The current region's `OpenSimExtras` `currency`.
    region: Option<String>,
    /// Whether the current region is an OpenSim one.
    opensim: bool,
}

impl GridCurrency {
    /// The symbol to show the grid's money with.
    #[must_use]
    pub fn symbol(&self) -> &str {
        named(self.region.as_deref())
            .or_else(|| named(self.login.as_deref()))
            .unwrap_or(if self.opensim {
                OPENSIM_SYMBOL
            } else {
                LINDEN_SYMBOL
            })
    }
}

/// `symbol` when it names one: trimmed, and not empty.
fn named(symbol: Option<&str>) -> Option<&str> {
    symbol.map(str::trim).filter(|symbol| !symbol.is_empty())
}

/// Keeps [`GridCurrency`] from the login's account facts and each region's
/// features.
pub fn ingest_grid_currency(
    mut events: MessageReader<SlEvent>,
    mut currency: ResMut<GridCurrency>,
) {
    for event in events.read() {
        let mut next = currency.clone();
        match &event.0 {
            SlSessionEvent::Account(account) => next.login.clone_from(&account.currency),
            SlSessionEvent::SimulatorFeatures(features) => {
                next.opensim = features.open_sim_extras.is_some();
                next.region = features
                    .open_sim_extras
                    .as_ref()
                    .and_then(|extras| extras.currency.clone());
            }
            _ => continue,
        }
        if *currency != next {
            *currency = next;
        }
    }
}

/// Registers [`GridCurrency`] and keeps it.
#[derive(Debug, Default)]
pub struct GridCurrencyPlugin;

impl Plugin for GridCurrencyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GridCurrency>()
            .add_systems(Update, ingest_grid_currency);
    }
}

#[cfg(test)]
mod tests {
    use super::{GridCurrency, LINDEN_SYMBOL, OPENSIM_SYMBOL};
    use pretty_assertions::assert_eq;

    /// Second Life names no symbol and is no OpenSim grid: `L$`.
    #[test]
    fn second_life_shows_lindens() {
        assert_eq!(GridCurrency::default().symbol(), LINDEN_SYMBOL);
    }

    /// An OpenSim region with no symbol of its own or the login's: `OS$`.
    #[test]
    fn an_opensim_grid_that_names_none_shows_os_dollars() {
        let currency = GridCurrency {
            opensim: true,
            ..GridCurrency::default()
        };
        assert_eq!(currency.symbol(), OPENSIM_SYMBOL);
    }

    /// The login's symbol beats the OpenSim default, and the region's beats
    /// the login's; an empty one names nothing.
    #[test]
    fn a_named_symbol_wins_region_first() {
        let mut currency = GridCurrency {
            login: Some("G$".to_owned()),
            region: Some(String::new()),
            opensim: true,
        };
        assert_eq!(currency.symbol(), "G$");
        currency.region = Some("R$".to_owned());
        assert_eq!(currency.symbol(), "R$");
    }
}
