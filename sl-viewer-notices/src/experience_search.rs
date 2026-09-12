//! The experience **search and row rendering** shared by every surface that
//! lists experiences.
//!
//! The reference viewer has one class for this — `LLPanelExperiencePicker` —
//! and embeds it twice: as the Experiences floater's Search tab
//! (`LLFloaterExperiences`) and as the modal "Choose Experience" window
//! (`LLFloaterExperiencePicker`) that every estate / parcel experience list
//! adds through. This module is the part of that class which is not a window:
//! the paging state a `FindExperienceByName` query moves through, the filters
//! the pickers are opened with, and the rendering of one experience id into the
//! rating / name / owner cells all three surfaces show.
//!
//! It exists so the picker is not a second search. [`crate::experiences_floater`]
//! and [`crate::experience_picker`] each own their window, their tables and
//! their own copy of the *results*; what they share is every function that turns
//! ids and [`ExperienceInfo`] records into rows, and the column set those rows
//! fill.
//!
//! Reference (Firestorm, read-only): `llpanelexperiencepicker.cpp`
//! (`processResponse`, `filterContent`, `FilterWithProperty`,
//! `FilterWithoutProperty`), `panel_experience_search.xml`.

use std::collections::BTreeMap;

use sl_client_bevy::{ExperienceInfo, ExperienceKey, ExperienceSearchPage, OwnerKey};

use crate::experience_profile::maturity_key;
use crate::i18n::Translator;
use crate::ui_table::{TableAlign, TableColumn, TableColumnKind, TableColumnWidth};
use crate::world_api::{AvatarState, ExperiencePickerFilter, GroupsModel};

/// How many leading hex characters of an experience id stand in for its name
/// while the name is still resolving.
const SHORT_ID_LEN: usize = 8;

/// The rating column of a search-shaped results table.
pub(crate) const COL_SEARCH_RATING: usize = 0;

/// The name column of a search-shaped results table.
pub(crate) const COL_SEARCH_NAME: usize = 1;

/// The owner column of a search-shaped results table.
pub(crate) const COL_SEARCH_OWNER: usize = 2;

/// The resolved-metadata cache both surfaces fold `GetExperienceInfo` replies
/// into, keyed by the id every other reply names an experience by.
pub(crate) type ExperienceInfos = BTreeMap<ExperienceKey, ExperienceInfo>;

/// The search results' columns — rating, name and owner, as the reference's
/// `search_results` scroll list.
pub(crate) static SEARCH_COLUMNS: [TableColumn; 3] = [
    TableColumn {
        header_key: "experiences-col-rating",
        token: "rating",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 84.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-name",
        token: "name",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-owner",
        token: "owner",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
];

/// A search's progress, which is what a results table's status line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SearchProgress {
    /// Nothing asked for yet.
    #[default]
    Idle,
    /// A query is out.
    Searching,
    /// A page came back, with the grid's word on what lies on either side of
    /// it — which is what enables the two arrows.
    Done {
        /// Whether the grid offered a page after this one.
        has_next_page: bool,
        /// Whether the grid offered a page before this one.
        has_previous_page: bool,
    },
}

impl SearchProgress {
    /// The progress an arrived page puts the search in: done, remembering the
    /// grid's two paging markers verbatim.
    pub(crate) const fn from_page(page: &ExperienceSearchPage) -> Self {
        Self::Done {
            has_next_page: page.has_next_page,
            has_previous_page: page.has_previous_page,
        }
    }

    /// Whether the Next arrow has somewhere to go. Only a page the grid said
    /// has a successor does — a search that has not answered yet has none, and
    /// neither does a grid that sends no markers.
    pub(crate) const fn offers_next(self) -> bool {
        matches!(
            self,
            Self::Done {
                has_next_page: true,
                ..
            }
        )
    }

    /// Whether the Previous arrow has somewhere to go.
    pub(crate) const fn offers_previous(self) -> bool {
        matches!(
            self,
            Self::Done {
                has_previous_page: true,
                ..
            }
        )
    }
}

/// One list / search row, with every cell already rendered for the current
/// locale and name caches — so the bind pass only writes strings, and a cache
/// that resolves later simply rebuilds the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExperienceRow {
    /// The experience this row stands for.
    pub(crate) id: ExperienceKey,
    /// The display name (or the short-id fallback).
    pub(crate) name: String,
    /// The content rating's label, or empty while the metadata is unknown.
    pub(crate) rating: String,
    /// The owner's resolved name, or empty when unknown.
    pub(crate) owner: String,
}

/// Render one list of ids into display rows.
pub(crate) fn render_rows(
    infos: &ExperienceInfos,
    ids: &[ExperienceKey],
    avatars: &AvatarState,
    groups: &GroupsModel,
    translator: &Translator,
) -> Vec<ExperienceRow> {
    ids.iter()
        .map(|id| {
            let info = infos.get(id);
            ExperienceRow {
                id: *id,
                name: experience_label(infos, *id),
                rating: info.map_or_else(String::new, |info| {
                    translator.get(maturity_key(info.maturity))
                }),
                owner: info
                    .and_then(|info| info.owner)
                    .map_or_else(String::new, |owner| owner_label(owner, avatars, groups)),
            }
        })
        .collect()
}

/// One owner's resolved name, or its id in parentheses until the cache has it.
pub(crate) fn owner_label(owner: OwnerKey, avatars: &AvatarState, groups: &GroupsModel) -> String {
    let resolved = match owner {
        OwnerKey::Agent(agent) => avatars.shown_name_of(agent).map(str::to_owned),
        OwnerKey::Group(group) => groups.group_name(group).map(str::to_owned),
    };
    resolved.unwrap_or_else(|| format!("({})", owner.uuid()))
}

/// The resolved name for an experience id, if known and non-empty.
pub(crate) fn experience_name(infos: &ExperienceInfos, id: ExperienceKey) -> Option<&str> {
    infos
        .get(&id)
        .map(|info| info.name.as_str())
        .filter(|name| !name.is_empty())
}

/// The row label for an experience: its resolved name, or the leading hex of
/// its id as a stable fallback while the name is still resolving.
pub(crate) fn experience_label(infos: &ExperienceInfos, id: ExperienceKey) -> String {
    experience_name(infos, id).map_or_else(|| short_experience_id(id), str::to_owned)
}

/// The leading [`SHORT_ID_LEN`] hex characters of an experience id, an ellipsis
/// appended — the stable fallback shown until the name resolves.
pub(crate) fn short_experience_id(id: ExperienceKey) -> String {
    let hex = id.uuid().simple().to_string();
    let head: String = hex.chars().take(SHORT_ID_LEN).collect();
    format!("{head}\u{2026}")
}

/// Order rendered experience rows by a table's sort keys, most significant
/// first. An unknown token leaves the order alone rather than inventing one.
pub(crate) fn sort_experience_rows(rows: &mut [ExperienceRow], keys: &[(&'static str, bool)]) {
    rows.sort_by(|left, right| {
        for (token, ascending) in keys {
            let ordering = match *token {
                "name" => compare_ci(&left.name, &right.name),
                "rating" => compare_ci(&left.rating, &right.rating),
                "owner" => compare_ci(&left.owner, &right.owner),
                _unknown => core::cmp::Ordering::Equal,
            };
            let ordering = if *ascending {
                ordering
            } else {
                ordering.reverse()
            };
            if ordering != core::cmp::Ordering::Equal {
                return ordering;
            }
        }
        core::cmp::Ordering::Equal
    });
}

/// Case-insensitive comparison, the way the reference's name comparator
/// upper-cases both sides before comparing.
pub(crate) fn compare_ci(left: &str, right: &str) -> core::cmp::Ordering {
    left.to_lowercase().cmp(&right.to_lowercase())
}

/// Whether a picker opened with `filter` may offer `id`.
///
/// A record the metadata cache has not resolved yet is **admitted**: the
/// reference's filters read properties off the cached record and a miss there
/// leaves the row in the list, so hiding an unresolved row would make the
/// visible result set depend on the order replies happen to arrive in.
pub(crate) fn filter_admits(
    filter: ExperiencePickerFilter,
    infos: &ExperienceInfos,
    id: ExperienceKey,
) -> bool {
    infos
        .get(&id)
        .is_none_or(|info| filter.admits(info.properties))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    use super::*;

    /// A row with just the fields the sort reads.
    fn row(name: &str, rating: &str, owner: &str) -> ExperienceRow {
        ExperienceRow {
            id: ExperienceKey::from(Uuid::from_u128(0x1)),
            name: name.to_owned(),
            rating: rating.to_owned(),
            owner: owner.to_owned(),
        }
    }

    /// The short id is the leading hex of the (dash-free) uuid with an ellipsis.
    #[test]
    fn short_id_is_leading_hex_with_ellipsis() {
        let id = ExperienceKey::from(Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0));
        let short = short_experience_id(id);
        assert_eq!(short, "12345678\u{2026}");
        assert_eq!(short.chars().count(), SHORT_ID_LEN + 1);
    }

    /// A row shows the resolved name once known, and the short-id fallback
    /// until then — and an *empty* name is not a resolution.
    #[test]
    fn label_prefers_the_resolved_name() {
        let id = ExperienceKey::from(Uuid::from_u128(0xabcd));
        let mut infos = ExperienceInfos::new();
        assert_eq!(experience_label(&infos, id), short_experience_id(id));

        // A record with an empty name is what the grid sends for an experience
        // it knows of but will not name; the fallback must survive it.
        let _empty = infos.insert(
            id,
            ExperienceInfo {
                public_id: id,
                name: String::new(),
                ..ExperienceInfo::default()
            },
        );
        assert_eq!(experience_label(&infos, id), short_experience_id(id));
        assert_eq!(experience_name(&infos, id), None);

        let _named = infos.insert(
            id,
            ExperienceInfo {
                public_id: id,
                name: "Neon Speedway".to_owned(),
                ..ExperienceInfo::default()
            },
        );
        assert_eq!(experience_label(&infos, id), "Neon Speedway");
    }

    /// The sort is case-insensitive, multi-level, and honours each level's
    /// direction.
    #[test]
    fn rows_sort_case_insensitively_by_each_key() {
        let mut rows = vec![
            row("beta", "General", "Zoe"),
            row("Alpha", "Moderate", "Ann"),
            row("alpha", "General", "Bob"),
        ];
        sort_experience_rows(&mut rows, &[("name", true), ("rating", true)]);
        assert_eq!(
            rows.iter().map(|r| r.rating.as_str()).collect::<Vec<_>>(),
            vec!["General", "Moderate", "General"]
        );
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "Alpha", "beta"]
        );

        sort_experience_rows(&mut rows, &[("owner", false)]);
        assert_eq!(
            rows.iter().map(|r| r.owner.as_str()).collect::<Vec<_>>(),
            vec!["Zoe", "Bob", "Ann"]
        );
    }

    /// An unknown sort token leaves the order alone rather than inventing one.
    #[test]
    fn an_unknown_sort_token_is_inert() {
        let mut rows = vec![row("beta", "", ""), row("alpha", "", "")];
        sort_experience_rows(&mut rows, &[("nonesuch", true)]);
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["beta", "alpha"]
        );
    }

    /// The two arrows are enabled from the grid's own markers, not from the
    /// row count: an empty page whose grid said there is more still offers a
    /// Next, and a page that fills the table but is the last one does not.
    #[test]
    fn the_arrows_follow_the_grids_markers() {
        let progress = |has_next_page, has_previous_page| {
            SearchProgress::from_page(&ExperienceSearchPage {
                infos: Vec::new(),
                has_next_page,
                has_previous_page,
            })
        };
        assert!(progress(true, false).offers_next());
        assert!(!progress(true, false).offers_previous());
        assert!(progress(false, true).offers_previous());
        assert!(!progress(false, true).offers_next());
        assert!(!progress(false, false).offers_next());
        assert!(!progress(false, false).offers_previous());
    }

    /// A search that has not answered offers no paging in either direction —
    /// there is no page to be on the far side of.
    #[test]
    fn an_unanswered_search_offers_no_paging() {
        for progress in [SearchProgress::Idle, SearchProgress::Searching] {
            assert!(!progress.offers_next(), "{progress:?}");
            assert!(!progress.offers_previous(), "{progress:?}");
        }
    }
}
