//! Land changing hands: a purchase, a deed, an abandonment, a reclaim and an
//! access pass — who may, what the parcel record reads after, and what a
//! refusal says. Measured by the conformance census `land-transactions`
//! (`book/src/gridspec/land.md` § Transactions, 2026-10-17).
//!
//! The rules here are the parcel's alone; [`crate::parcel_edits`] answers the
//! messages, pushes the changed record and re-sends the overlay. Nothing here
//! moves money: the fake grid keeps no ledger (`server-fake-grid-money-ledger`),
//! so a sale and a pass are free, as a stock OpenSim's are.

use sl_proto::{ParcelAccessEntry, ParcelAccessFlags, ParcelFlags, ParcelInfo, ParcelStatus};
use sl_types::key::{AgentKey, GroupKey, OwnerKey};
use sl_types::money::LindenAmount;

use crate::imitates::ParcelPolicy;

/// The flags a change of hands takes off a parcel: on sale (land and its
/// objects) and listed in search — OpenSim's `UpdateLandSold`, `DeedToGroup`,
/// abandon and reclaim clear the same four.
const SALE_FLAGS: u32 = ParcelFlags::FOR_SALE.bits()
    | FOR_SALE_OBJECTS
    | SELL_PARCEL_OBJECTS
    | ParcelFlags::SHOW_DIRECTORY.bits();

/// `PF_FOR_SALE_OBJECTS`: the parcel is for sale with its objects.
const FOR_SALE_OBJECTS: u32 = 1 << 7;

/// `PF_SELL_PARCEL_OBJECTS`: the objects go to the buyer.
const SELL_PARCEL_OBJECTS: u32 = 1 << 14;

/// The access-list entry flag a pass is written under (`AL_ACCESS`).
const PASS_ENTRY: ParcelAccessFlags = ParcelAccessFlags(1);

/// Why a purchase is not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuyRefused {
    /// The parcel is not for sale.
    NotForSale,
    /// It is for sale to somebody else.
    ToSomebodyElse,
    /// The offer is below the price.
    TooLittle,
}

/// Whether `buyer` may buy `parcel` at `offered`: it is for sale, to anyone
/// or to the buyer, and the offer meets the price — OpenSim's
/// `EventManagerOnValidateLandBuy`.
pub(crate) fn check_buy(
    parcel: &ParcelInfo,
    buyer: AgentKey,
    offered: &LindenAmount,
) -> Result<(), BuyRefused> {
    if !parcel.flags().contains(ParcelFlags::FOR_SALE) {
        return Err(BuyRefused::NotForSale);
    }
    if parcel
        .auth_buyer_id
        .is_some_and(|authorised| authorised != buyer)
    {
        return Err(BuyRefused::ToSomebodyElse);
    }
    let price = parcel.sale_price.as_ref().map_or(0, |price| price.0);
    if offered.0 < price {
        return Err(BuyRefused::TooLittle);
    }
    Ok(())
}

/// Hands `parcel` to `owner` with `group` as its group: off the market, no
/// longer listed. OpenSim writes the group the buy names — the buyer's
/// active group, as the reference viewer sends it — whether or not the
/// purchase is for that group.
pub(crate) const fn change_hands(
    parcel: &mut ParcelInfo,
    owner: OwnerKey,
    group: Option<GroupKey>,
) {
    parcel.owner = owner;
    parcel.group = group;
    parcel.status = ParcelStatus::Leased;
    parcel.sale_price = Some(LindenAmount(0));
    parcel.auth_buyer_id = None;
    parcel.claim_price = LindenAmount(0);
    parcel.raw_parcel_flags &= !SALE_FLAGS;
}

/// Whether `agent` holds `parcel` itself — who may deed or abandon it.
pub(crate) fn holds(parcel: &ParcelInfo, agent: AgentKey) -> bool {
    parcel.owner == OwnerKey::Agent(agent)
}

/// Gives `parcel` back to the estate (`ParcelRelease`): the estate owner holds
/// it, with no group. OpenSim leaves its status alone; Second Life's is
/// unmeasured and keeps the reference viewer's `OS_ABANDONED`.
pub(crate) const fn abandon(parcel: &mut ParcelInfo, estate_owner: AgentKey, policy: ParcelPolicy) {
    let status = if policy.abandon_marks_abandoned {
        ParcelStatus::Abandoned
    } else {
        parcel.status
    };
    change_hands(parcel, OwnerKey::Agent(estate_owner), None);
    parcel.status = status;
}

/// Takes `parcel` back into the estate (`ParcelReclaim`): the estate owner
/// holds it, with no group, and it shows its avatars and their sounds again —
/// OpenSim's `ClientOnParcelReclaim`.
pub(crate) const fn reclaim(parcel: &mut ParcelInfo, estate_owner: AgentKey) {
    change_hands(parcel, OwnerKey::Agent(estate_owner), None);
    parcel.see_avs = Some(true);
    parcel.any_av_sounds = Some(true);
    parcel.group_av_sounds = Some(true);
}

/// The answer to a pass purchase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PassAnswer {
    /// Sold: the access-list entry it writes, replacing the buyer's own, and
    /// what the buyer is told (`None`: nothing).
    Sold {
        /// The new entry.
        entry: ParcelAccessEntry,
        /// The agent alert the buyer is sent, if any.
        told: Option<&'static str>,
    },
    /// Not sold; the agent alert the buyer is sent, if any.
    Refused(Option<String>),
}

/// The answer to `buyer`'s pass to `parcel` at the Unix time `now`, given the
/// parcel's allow list — OpenSim's `ClientParcelBuyPass`, with the two things
/// Second Life was measured to do otherwise ([`ParcelPolicy::pass_needs_pass_list`],
/// [`ParcelPolicy::pass_sale_told`]).
pub(crate) fn sell_pass(
    parcel: &ParcelInfo,
    allowed: &[ParcelAccessEntry],
    buyer: AgentKey,
    now: i32,
    policy: ParcelPolicy,
) -> PassAnswer {
    if parcel.owner == OwnerKey::Agent(buyer) || parcel.pass_hours <= 0.0 {
        return PassAnswer::Refused(None);
    }
    if matches!(parcel.owner, OwnerKey::Group(_)) && policy.pass_needs_pass_list {
        return PassAnswer::Refused(Some("/pass to group owned parcel not suported".to_owned()));
    }
    if policy.pass_needs_pass_list && !parcel.flags().contains(ParcelFlags::USE_PASS_LIST) {
        return PassAnswer::Refused(None);
    }
    let length = seconds_of(parcel.pass_hours);
    let held = allowed
        .iter()
        .find(|entry| entry.id == buyer.uuid() && entry.flags == PASS_ENTRY);
    let mut expires = now.saturating_add(length);
    if let Some(entry) = held {
        if entry.time == 0 {
            return PassAnswer::Refused(Some("/You already have access to parcel".to_owned()));
        }
        let left = entry.time.saturating_sub(now);
        if left > length / 4 {
            return PassAnswer::Refused(Some(format!(
                "/You already have a pass valid for {}",
                time_left(left)
            )));
        }
        if left > 0 {
            expires = expires.saturating_add(left);
        }
    }
    PassAnswer::Sold {
        entry: ParcelAccessEntry {
            id: buyer.uuid(),
            time: expires,
            flags: PASS_ENTRY,
        },
        told: policy.pass_sale_told,
    }
}

/// A pass's length in whole seconds, rounded as OpenSim rounds it.
fn seconds_of(hours: f32) -> i32 {
    let seconds = (f64::from(hours) * 3600.0).round();
    format!("{seconds:.0}").parse().unwrap_or(i32::MAX)
}

/// The time a pass has left, as OpenSim words it: hours to three places,
/// minutes to two, seconds to one, each without trailing zeros.
fn time_left(seconds: i32) -> String {
    let whole = f64::from(seconds);
    if seconds > 3600 {
        format!("{} hours", trimmed(whole / 3600.0, 3))
    } else if seconds > 60 {
        format!("{} minutes", trimmed(whole / 60.0, 2))
    } else {
        format!("{} seconds", trimmed(whole, 1))
    }
}

/// `value` to at most `places` decimals, trailing zeros and point dropped —
/// .NET's `0.##` format.
fn trimmed(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::{ParcelAccessEntry, ParcelFlags, ParcelInfo, RegionLocalParcelId};
    use sl_types::key::{AgentKey, GroupKey, OwnerKey};
    use sl_types::money::LindenAmount;

    use super::{
        BuyRefused, PASS_ENTRY, PassAnswer, abandon, change_hands, check_buy, reclaim, sell_pass,
        time_left,
    };
    use crate::imitates::ImitatedGrid;
    use crate::world::region_wide_parcel;

    /// The plot's owner.
    fn owner() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(1))
    }

    /// The resident buying from it.
    fn buyer() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(2))
    }

    /// The estate's owner.
    fn estate() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(3))
    }

    /// A plot of [`owner`]'s, not for sale.
    fn plot() -> ParcelInfo {
        region_wide_parcel(RegionLocalParcelId(1), OwnerKey::Agent(owner()), "Plot")
    }

    /// The plot for sale at `price` to `to`.
    fn for_sale(price: u64, to: Option<AgentKey>) -> ParcelInfo {
        let mut parcel = plot();
        parcel.raw_parcel_flags |= ParcelFlags::FOR_SALE.bits();
        parcel.sale_price = Some(LindenAmount(price));
        parcel.auth_buyer_id = to;
        parcel
    }

    /// A purchase is made only of land for sale, to anyone or the buyer, at
    /// its price or more — as OpenSim validates it.
    #[test]
    fn a_purchase_needs_land_for_sale_to_the_buyer_at_its_price() {
        let one = LindenAmount(1);
        assert_eq!(
            check_buy(&plot(), buyer(), &one),
            Err(BuyRefused::NotForSale)
        );
        assert_eq!(
            check_buy(&for_sale(1, Some(estate())), buyer(), &one),
            Err(BuyRefused::ToSomebodyElse)
        );
        assert_eq!(
            check_buy(&for_sale(1, Some(buyer())), buyer(), &LindenAmount(0)),
            Err(BuyRefused::TooLittle)
        );
        assert_eq!(
            check_buy(&for_sale(1, Some(buyer())), buyer(), &one),
            Ok(())
        );
        assert_eq!(
            check_buy(&for_sale(0, None), buyer(), &LindenAmount(0)),
            Ok(())
        );
    }

    /// Land that changes hands comes off the market and takes the group the
    /// purchase names, as OpenSim writes the buyer's active group.
    #[test]
    fn land_that_changes_hands_is_off_the_market_under_the_named_group() {
        let group = GroupKey::from(uuid::Uuid::from_u128(9));
        let mut parcel = for_sale(1, Some(buyer()));
        parcel.raw_parcel_flags |= ParcelFlags::SHOW_DIRECTORY.bits();
        change_hands(&mut parcel, OwnerKey::Agent(buyer()), Some(group));
        assert_eq!(parcel.owner, OwnerKey::Agent(buyer()));
        assert_eq!(parcel.group, Some(group));
        assert!(!parcel.flags().contains(ParcelFlags::FOR_SALE));
        assert!(!parcel.flags().contains(ParcelFlags::SHOW_DIRECTORY));
        assert_eq!(parcel.auth_buyer_id, None);
        assert_eq!(parcel.sale_price, Some(LindenAmount(0)));
    }

    /// Abandoned land goes to the estate owner; OpenSim leaves it leased.
    #[test]
    fn abandoned_land_goes_to_the_estate_owner() {
        for (imitates, abandoned) in [
            (ImitatedGrid::OpenSim, false),
            (ImitatedGrid::SecondLife, true),
        ] {
            let mut parcel = plot();
            abandon(&mut parcel, estate(), imitates.parcel_policy());
            assert_eq!(parcel.owner, OwnerKey::Agent(estate()), "{imitates:?}");
            assert_eq!(
                parcel.status == sl_proto::ParcelStatus::Abandoned,
                abandoned,
                "{imitates:?}"
            );
        }
        let mut parcel = plot();
        parcel.see_avs = Some(false);
        reclaim(&mut parcel, estate());
        assert_eq!(parcel.owner, OwnerKey::Agent(estate()));
        assert_eq!(parcel.see_avs, Some(true));
    }

    /// A pass on OpenSim needs the pass list on, and a second one while the
    /// first has most of its time left is refused with the time left; Second
    /// Life sells one off the price alone and says so.
    #[test]
    fn a_pass_is_sold_as_each_grid_sells_it() -> Result<(), String> {
        let now = 1_000_000;
        let mut parcel = plot();
        parcel.pass_price = LindenAmount(1);
        parcel.pass_hours = 1.0;
        let opensim = ImitatedGrid::OpenSim.parcel_policy();
        let second_life = ImitatedGrid::SecondLife.parcel_policy();
        assert_eq!(
            sell_pass(&parcel, &[], buyer(), now, opensim),
            PassAnswer::Refused(None)
        );
        assert_eq!(
            sell_pass(&parcel, &[], buyer(), now, second_life),
            PassAnswer::Sold {
                entry: ParcelAccessEntry {
                    id: buyer().uuid(),
                    time: now + 3600,
                    flags: PASS_ENTRY,
                },
                told: Some("You purchased a pass."),
            }
        );
        parcel.raw_parcel_flags |= ParcelFlags::USE_PASS_LIST.bits();
        let PassAnswer::Sold { entry, told } = sell_pass(&parcel, &[], buyer(), now, opensim)
        else {
            return Err("OpenSim refused a pass the list allows".to_owned());
        };
        assert_eq!(told, None);
        assert_eq!(
            sell_pass(&parcel, &[entry], buyer(), now + 8, opensim),
            PassAnswer::Refused(Some(
                "/You already have a pass valid for 59.87 minutes".to_owned()
            ))
        );
        assert_eq!(
            sell_pass(&parcel, &[], owner(), now, opensim),
            PassAnswer::Refused(None)
        );
        Ok(())
    }

    /// The time left is worded as OpenSim words it.
    #[test]
    fn the_time_left_is_worded_as_opensim_words_it() {
        assert_eq!(time_left(3592), "59.87 minutes");
        assert_eq!(time_left(7200 + 36), "2.01 hours");
        assert_eq!(time_left(30), "30 seconds");
    }
}
