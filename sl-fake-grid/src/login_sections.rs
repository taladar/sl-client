//! The login response's content sections, per flavour, as each live grid was
//! measured sending them (`login-options` on aditi and the local OpenSim,
//! 2026-10-04; `book/src/gridspec/login.md`).
//!
//! Which sections a response carries at all is [`LoginFields`]'s business, and
//! the `options` filter trims a Second-Life-flavoured response to what was
//! asked for afterwards. This module fills them in: the category lists, the
//! sky's global textures, the login flags, the UI config, the initial outfit,
//! the tutorial setting and the scalars only one grid sends.
//!
//! [`LoginFields`]: crate::imitates::LoginFields

use sl_types::key::{AgentKey, TextureKey};
use sl_wire::{
    GlobalTextures, InitialOutfit, LoginCategory, LoginFlags, LoginList, LoginSuccess,
    TutorialSetting, UiConfig,
};
use uuid::Uuid;

/// The classified-ad categories, which both grids send identically.
const CLASSIFIED_CATEGORIES: &[(i32, &str)] = &[
    (1, "Shopping"),
    (2, "Land Rental"),
    (3, "Property Rental"),
    (4, "Special Attraction"),
    (5, "New Products"),
    (6, "Employment"),
    (7, "Wanted"),
    (8, "Service"),
    (9, "Personal"),
];

/// Second Life's event categories. A stock OpenSim sends the list empty.
const SECOND_LIFE_EVENT_CATEGORIES: &[(i32, &str)] = &[
    (18, "Discussion"),
    (19, "Sports"),
    (20, "Live Music"),
    (22, "Commercial"),
    (23, "Nightlife/Entertainment"),
    (24, "Games/Contests"),
    (25, "Pageants"),
    (26, "Education"),
    (27, "Arts and Culture"),
    (28, "Charity/Support Groups"),
    (29, "Miscellaneous"),
    (30, "Live DJ"),
    (31, "Spirituality"),
];

/// The default sun texture, which both grids name.
const SUN_TEXTURE: u128 = 0xcce0_f112_878f_4586_a2e2_a8f1_04bb_a271;

/// Second Life's moon texture.
const SECOND_LIFE_MOON_TEXTURE: u128 = 0xd07f_6eed_b96a_47cd_b51d_400a_d4a1_c428;

/// Second Life's cloud texture.
const SECOND_LIFE_CLOUD_TEXTURE: u128 = 0xfc4b_9f0b_d008_45c6_96a4_01dd_947a_c621;

/// OpenSim's moon texture.
const OPENSIM_MOON_TEXTURE: u128 = 0xec4b_9f0b_d008_45c6_96a4_01dd_947a_c621;

/// OpenSim's cloud texture.
const OPENSIM_CLOUD_TEXTURE: u128 = 0xdc4b_9f0b_d008_45c6_96a4_01dd_947a_c621;

/// Second Life's tutorial page.
const SECOND_LIFE_TUTORIAL_URL: &str = "http://help.secondlife.com/orientation/";

/// The UDP messages Second Life names in `udp_blacklist`: the four a region
/// hands over through the event queue instead.
const SECOND_LIFE_UDP_BLACKLIST: [&str; 4] = [
    "EnableSimulator",
    "TeleportFinish",
    "CrossedRegion",
    "OpenCircuit",
];

/// The initial outfit a stock OpenSim names.
const OPENSIM_INITIAL_OUTFIT: (&str, &str) = ("Nightclub Female", "female");

/// The content sections of a login response, per flavour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginSections {
    /// The event categories (empty: sent as an empty array).
    event_categories: &'static [(i32, &'static str)],
    /// The moon and cloud textures of `global-textures`.
    moon_and_cloud: (u128, u128),
    /// The initial outfit: `None` for Second Life's empty struct.
    initial_outfit: Option<(&'static str, &'static str)>,
    /// The tutorial page, when the grid names one.
    tutorial_url: Option<&'static str>,
    /// Whether the Second Life-only scalars are sent (`agent_flags`,
    /// `cof_version`, the god levels, `is_admin_login`, `Linden_Status_Code`,
    /// `udp_blacklist`).
    second_life_scalars: bool,
    /// Whether the OpenSim-only scalars are sent (`http_port`, `real_id`).
    opensim_scalars: bool,
    /// The list sections sent as an empty array when they hold nothing.
    empty_lists: &'static [LoginList],
}

impl LoginSections {
    /// What Second Life sends.
    pub const SECOND_LIFE: Self = Self {
        event_categories: SECOND_LIFE_EVENT_CATEGORIES,
        moon_and_cloud: (SECOND_LIFE_MOON_TEXTURE, SECOND_LIFE_CLOUD_TEXTURE),
        initial_outfit: None,
        tutorial_url: Some(SECOND_LIFE_TUTORIAL_URL),
        second_life_scalars: true,
        opensim_scalars: false,
        // Second Life leaves an empty list out: an account with no friends gets
        // no `buddy-list`, one with a friend gets it (`login-buddy-list`).
        empty_lists: &[],
    };

    /// What a stock OpenSim sends.
    pub const OPENSIM: Self = Self {
        event_categories: &[],
        moon_and_cloud: (OPENSIM_MOON_TEXTURE, OPENSIM_CLOUD_TEXTURE),
        initial_outfit: Some(OPENSIM_INITIAL_OUTFIT),
        tutorial_url: None,
        second_life_scalars: false,
        opensim_scalars: true,
        empty_lists: &[
            LoginList::BuddyList,
            LoginList::Gestures,
            LoginList::EventCategories,
            LoginList::EventNotifications,
        ],
    };

    /// Fill `success`'s content sections the way this flavour does, at UNIX
    /// time `now`, for a login whose Current Outfit Folder is at
    /// `cof_version`.
    pub(crate) fn fill(self, success: &mut LoginSuccess, now: i64, cof_version: i32) {
        success.classified_categories = categories(CLASSIFIED_CATEGORIES);
        success.event_categories = categories(self.event_categories);
        let (moon, cloud) = self.moon_and_cloud;
        success.global_textures = Some(GlobalTextures {
            sun_texture_id: TextureKey::from(Uuid::from_u128(SUN_TEXTURE)),
            cloud_texture_id: TextureKey::from(Uuid::from_u128(cloud)),
            moon_texture_id: TextureKey::from(Uuid::from_u128(moon)),
        });
        // Both grids answered these four alike; only `daylight_savings`
        // moves, with Pacific time, which is the grids' own clock.
        success.login_flags = Some(LoginFlags {
            ever_logged_in: true,
            daylight_savings: pacific_daylight_saving(now),
            gendered: true,
            stipend_since_login: "N".to_owned(),
        });
        success.ui_config = Some(UiConfig {
            allow_first_life: true,
        });
        success.initial_outfit = Some(self.initial_outfit.map_or_else(
            || InitialOutfit {
                folder_name: String::new(),
                gender: String::new(),
            },
            |(folder_name, gender)| InitialOutfit {
                folder_name: folder_name.to_owned(),
                gender: gender.to_owned(),
            },
        ));
        // Second Life sends the URL and an empty `use_tutorial` as two entries.
        success.tutorial_settings = self.tutorial_url.map_or_else(Vec::new, |url| {
            vec![
                TutorialSetting {
                    tutorial_url: Some(url.to_owned()),
                    use_tutorial: None,
                },
                TutorialSetting {
                    tutorial_url: None,
                    use_tutorial: Some(String::new()),
                },
            ]
        });
        if self.second_life_scalars {
            success.agent_flags = Some(0);
            success.cof_version = Some(cof_version);
            success.god_level = Some(0);
            success.max_god_level = Some(0);
            success.is_admin_login = Some(false);
            // A request trace id: unique per login, in Second Life's shape.
            success.linden_status_code = Some(format!(
                "1-{:08x}-{}",
                now.unsigned_abs() & 0xffff_ffff,
                success
                    .session_id
                    .simple()
                    .to_string()
                    .chars()
                    .take(24)
                    .collect::<String>()
            ));
            success.udp_blacklist = SECOND_LIFE_UDP_BLACKLIST
                .iter()
                .map(|&name| name.to_owned())
                .collect();
        }
        if self.opensim_scalars {
            success.http_port = Some(0);
            success.real_id = Some(AgentKey::from(Uuid::nil()));
        }
        success.empty_lists = self
            .empty_lists
            .iter()
            .copied()
            .filter(|list| list.is_empty_in(success))
            .collect();
    }
}

/// `entries` as login categories.
fn categories(entries: &[(i32, &str)]) -> Vec<LoginCategory> {
    entries
        .iter()
        .map(|&(category_id, name)| LoginCategory {
            category_id,
            category_name: name.to_owned(),
        })
        .collect()
}

/// Whether US Pacific daylight-saving time is in effect at UNIX time `now`:
/// from 02:00 local on the second Sunday of March (10:00 UTC) to 02:00 local
/// on the first Sunday of November (09:00 UTC).
fn pacific_daylight_saving(now: i64) -> bool {
    let Ok(at) = ::time::OffsetDateTime::from_unix_timestamp(now) else {
        return false;
    };
    let year = at.year();
    let start = nth_sunday(year, ::time::Month::March, 2).map(|day| day.with_hms(10, 0, 0));
    let end = nth_sunday(year, ::time::Month::November, 1).map(|day| day.with_hms(9, 0, 0));
    match (start, end) {
        (Some(Ok(start)), Some(Ok(end))) => {
            let at = ::time::PrimitiveDateTime::new(at.date(), at.time());
            start <= at && at < end
        }
        _unrepresentable => false,
    }
}

/// The `nth` Sunday (1-based) of `month` in `year`.
fn nth_sunday(year: i32, month: ::time::Month, nth: u8) -> Option<::time::Date> {
    ::time::Date::from_calendar_date(year, month, 1)
        .ok()
        .and_then(|first| {
            let offset = first.weekday().number_days_from_sunday();
            let first_sunday = if offset == 0 {
                1
            } else {
                8_u8.checked_sub(offset)?
            };
            let day = first_sunday.checked_add(nth.checked_sub(1)?.checked_mul(7)?)?;
            ::time::Date::from_calendar_date(year, month, day).ok()
        })
}

#[cfg(test)]
mod tests {
    use super::{LoginSections, pacific_daylight_saving};
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use sl_wire::{LoginList, LoginSuccess};
    use uuid::Uuid;

    /// The measured login date (2026-10-04) is daylight-saving time; a
    /// January date is not, and the two changeovers of 2026 (8 March,
    /// 1 November) fall on the right side of their hour.
    #[test]
    fn pacific_daylight_saving_follows_the_us_rule() {
        // 2026-10-04 12:00 UTC.
        assert!(pacific_daylight_saving(1_791_115_200));
        // 2026-01-15 12:00 UTC.
        assert!(!pacific_daylight_saving(1_768_478_400));
        // 2026-03-08 09:59 and 10:00 UTC.
        assert!(!pacific_daylight_saving(1_772_963_940));
        assert!(pacific_daylight_saving(1_772_964_000));
        // 2026-11-01 08:59 and 09:00 UTC.
        assert!(pacific_daylight_saving(1_793_523_540));
        assert!(!pacific_daylight_saving(1_793_523_600));
    }

    /// Filling a response gives each flavour the sections its grid was
    /// measured sending, and only those.
    #[test]
    fn each_flavour_fills_what_its_grid_sends() -> Result<(), String> {
        let blank = || -> Result<LoginSuccess, String> {
            Ok(LoginSuccess::minimal(
                AgentKey::from(Uuid::nil()),
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                sl_wire::CircuitCode(3),
                std::net::Ipv4Addr::LOCALHOST,
                9000,
                "http://127.0.0.1:9000/seed"
                    .parse()
                    .map_err(|error| format!("{error}"))?,
            ))
        };
        let mut second_life = blank()?;
        LoginSections::SECOND_LIFE.fill(&mut second_life, 1_791_115_200, 4);
        assert_eq!(second_life.event_categories.len(), 13);
        assert_eq!(second_life.udp_blacklist.len(), 4);
        assert_eq!(second_life.cof_version, Some(4));
        assert_eq!(second_life.http_port, None);
        assert_eq!(second_life.empty_lists, Vec::new());
        assert_eq!(second_life.tutorial_settings.len(), 2);

        let mut opensim = blank()?;
        LoginSections::OPENSIM.fill(&mut opensim, 1_791_115_200, 4);
        assert_eq!(opensim.event_categories.len(), 0);
        assert_eq!(opensim.classified_categories.len(), 9);
        assert_eq!(opensim.cof_version, None);
        assert_eq!(opensim.http_port, Some(0));
        assert_eq!(
            opensim.empty_lists,
            vec![
                LoginList::BuddyList,
                LoginList::Gestures,
                LoginList::EventCategories,
                LoginList::EventNotifications,
            ]
        );
        assert_eq!(
            opensim
                .initial_outfit
                .as_ref()
                .map(|outfit| outfit.folder_name.as_str()),
            Some("Nightclub Female")
        );
        Ok(())
    }
}
