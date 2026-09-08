//! The subscription packages a Second-Life-flavoured grid describes at login.
//!
//! Second Life's login response says what an account may do, and what it
//! *would* be able to do on every other subscription tier: `account_type` names
//! the package it is on, `account_level_benefits` carries that package's
//! numbers, and `premium_packages` carries all of them so a viewer can render
//! the comparison. A stock OpenSim grid sends none of the three
//! ([`ImitatedGrid::describes_account_entitlements`](crate::ImitatedGrid::describes_account_entitlements)).
//!
//! # The table, measured
//!
//! Every number below is one aditi login, recorded by `sl-conformance`'s
//! `login-handshake` case on 2026-09-08. Nothing here is inferred from the
//! viewer source or from what the tiers are advertised to cost.
//!
//! | package | texture | 2K texture | sound | animation | group | groups | animesh | picks | attachments |
//! | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
//! | `Base` | 10 | 50 | 10 | 10 | 100 | 50 | 1 | 20 | 38 |
//! | `Plus` | 10 | 50 | 10 | 10 | 100 | 55 | 1 | 20 | 38 |
//! | `Premium` | 10 | **40** | 10 | 10 | 100 | 80 | 2 | 20 | 38 |
//! | `Premium_Plus` | **0** | **0** | **0** | **0** | **10** | 150 | 3 | 20 | 38 |
//! | `Premium_Plus_No_Stipend` | 0 | 0 | 0 | 0 | 10 | 150 | 3 | 20 | 38 |
//!
//! **What a subscription actually buys** turns out to be worth having written
//! down, because two thirds of the table does not move. Uploads are free on the
//! two Premium Plus tiers and full price on everything below; the large-texture
//! tier is the only price that steps *gradually* (50, 50, 40, 0); group
//! creation drops from L$ 100 to L$ 10 only at Premium Plus; and the group
//! limit is the one figure that rises at every tier (50, 55, 80, 150, 150).
//! `picks_limit` and `attachment_limit` are identical across all five, so a
//! viewer gating either on the subscription would be gating on nothing.
//!
//! **The 2K tier is the reason this module exists at all.** A 2048×2048 texture
//! costs L$ 50 on `Base` against L$ 10 for anything at or below 1024×1024, and
//! `EconomyData` — which quotes a flat `price_upload` of 10 — has no way to say
//! so. A client that sent the legacy figure as its `expected_upload_cost` for a
//! large texture would be refused, and would have no idea why.
//!
//! **Five packages, not the two the viewer demands.** The reference viewer
//! requires `Base` and `Premium` to be present and complains at startup when
//! either is missing; aditi sends five. The fake grid sends all five, because
//! the shape a viewer meets on the real grid includes tiers it has no special
//! knowledge of, and a grid that sent exactly the required two would never
//! exercise that.

use std::collections::BTreeMap;

use sl_proto::AccountBenefits;
use sl_types::money::LindenAmount;

/// The package a [`FakeGridBuilder`](crate::FakeGridBuilder) puts an account on
/// unless [`AccountConfig::package`](crate::AccountConfig::package) says
/// otherwise, and the one the test avatar is really on.
pub const DEFAULT_PACKAGE: &str = "Base";

/// One row of the measured table.
///
/// A private tuple rather than a public constructor: these are measurements,
/// and the way to get a different set of numbers is
/// [`FakeGridBuilder::packages`](crate::FakeGridBuilder::packages) with an
/// [`AccountBenefits`] built directly, not a half-filled row from here.
fn row(
    upload: u64,
    large_texture: u64,
    create_group: u64,
    groups: i32,
    animesh: i32,
) -> AccountBenefits {
    AccountBenefits {
        animated_object_limit: animesh,
        animation_upload_cost: LindenAmount(upload),
        // Identical on all five measured tiers, so it is a constant here rather
        // than a parameter that would imply it varies.
        attachment_limit: 38,
        create_group_cost: LindenAmount(create_group),
        group_membership_limit: groups,
        picks_limit: 20,
        sound_upload_cost: LindenAmount(upload),
        texture_upload_cost: LindenAmount(upload),
        large_texture_upload_costs: vec![LindenAmount(large_texture)],
    }
}

/// The five subscription packages Second Life described on aditi, by name.
///
/// See the [module docs](self) for the table and where it came from.
#[must_use]
pub fn second_life_packages() -> BTreeMap<String, AccountBenefits> {
    [
        ("Base", row(10, 50, 100, 50, 1)),
        ("Plus", row(10, 50, 100, 55, 1)),
        ("Premium", row(10, 40, 100, 80, 2)),
        ("Premium_Plus", row(0, 0, 10, 150, 3)),
        ("Premium_Plus_No_Stipend", row(0, 0, 10, 150, 3)),
    ]
    .into_iter()
    .map(|(name, benefits)| (name.to_owned(), benefits))
    .collect()
}

/// Encodes one package back into the LLSD map a login response carries.
///
/// The inverse of [`AccountBenefits::from_llsd`], and it writes **every** field
/// that decode requires: the reference parse abandons a whole package on the
/// first missing one, so a partial map here would be indistinguishable to a
/// viewer from a grid that sent nothing.
#[must_use]
pub fn to_llsd(benefits: &AccountBenefits) -> sl_wire::Llsd {
    let integer = |value: i32| sl_wire::Llsd::Integer(value);
    let amount =
        |value: &LindenAmount| sl_wire::Llsd::Integer(i32::try_from(value.0).unwrap_or(i32::MAX));
    sl_wire::Llsd::Map(
        [
            (
                "animated_object_limit",
                integer(benefits.animated_object_limit),
            ),
            (
                "animation_upload_cost",
                amount(&benefits.animation_upload_cost),
            ),
            ("attachment_limit", integer(benefits.attachment_limit)),
            ("create_group_cost", amount(&benefits.create_group_cost)),
            (
                "group_membership_limit",
                integer(benefits.group_membership_limit),
            ),
            ("picks_limit", integer(benefits.picks_limit)),
            ("sound_upload_cost", amount(&benefits.sound_upload_cost)),
            ("texture_upload_cost", amount(&benefits.texture_upload_cost)),
            (
                "large_texture_upload_cost",
                sl_wire::Llsd::Array(
                    benefits
                        .large_texture_upload_costs
                        .iter()
                        .map(amount)
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect(),
    )
}

/// Encodes the whole package table as a login response's `premium_packages`,
/// each entry nesting its numbers under `benefits` the way the live grid does.
#[must_use]
pub fn packages_to_llsd(packages: &BTreeMap<String, AccountBenefits>) -> sl_wire::Llsd {
    sl_wire::Llsd::Map(
        packages
            .iter()
            .map(|(name, benefits)| {
                (
                    name.clone(),
                    sl_wire::Llsd::Map(
                        [("benefits".to_owned(), to_llsd(benefits))]
                            .into_iter()
                            .collect(),
                    ),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_types::money::LindenAmount;

    use super::{DEFAULT_PACKAGE, second_life_packages};

    /// The two packages the reference viewer refuses to start without are both
    /// present, and so is the one the default account is on.
    ///
    /// `LLAgentBenefitsMgr` warns and fails its init when `Base` or `Premium` is
    /// missing, so a grid without them makes a real viewer complain at every
    /// login — a failure that would show up as a notification nobody connected
    /// to this table.
    #[test]
    fn the_packages_a_viewer_insists_on_are_there() {
        let packages = second_life_packages();
        assert!(packages.contains_key("Base"), "no Base package");
        assert!(packages.contains_key("Premium"), "no Premium package");
        assert!(
            packages.contains_key(DEFAULT_PACKAGE),
            "the default account is on a package this grid does not describe"
        );
    }

    /// A large texture costs more than a small one on every tier that charges
    /// for uploads at all.
    ///
    /// This is the whole reason the benefits package cannot be collapsed into
    /// `EconomyData`'s flat `price_upload`, so it is worth an assertion rather
    /// than a comment: a table that lost the distinction would still look
    /// plausible and would price every 2048×2048 upload wrongly.
    #[test]
    fn a_large_texture_costs_more_wherever_uploads_cost_anything() {
        for (name, benefits) in second_life_packages() {
            let small = benefits.texture_upload_cost.clone();
            let large = benefits.large_texture_upload_cost();
            if small == LindenAmount(0) {
                assert_eq!(large, LindenAmount(0), "{name} charges for large only");
            } else {
                assert!(
                    large > small,
                    "{name} does not charge more for a 2K texture"
                );
            }
            // And the dispatch agrees with the table either side of the tier.
            assert_eq!(benefits.texture_upload_cost_for(1024, 1024), small);
            assert_eq!(benefits.texture_upload_cost_for(2048, 2048), large);
        }
    }

    /// Every tier grants at least as much as the one below it.
    ///
    /// Checked on the figure that actually moves at all five tiers. A table
    /// mistyped so that a paid package granted *less* than the free one would
    /// be the kind of wrong that reads fine.
    #[test]
    fn each_tier_grants_at_least_the_one_below() {
        let packages = second_life_packages();
        let limit = |name: &str| {
            packages
                .get(name)
                .map_or(0, |benefits| benefits.group_membership_limit)
        };
        assert!(limit("Base") <= limit("Plus"));
        assert!(limit("Plus") <= limit("Premium"));
        assert!(limit("Premium") <= limit("Premium_Plus"));
    }
}
