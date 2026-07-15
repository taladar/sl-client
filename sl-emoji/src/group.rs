//! The top-level Unicode emoji groups (the picker's tabs).

use crate::emoji::Emoji;

/// A top-level Unicode emoji group — the categories a picker shows as tabs, in
/// their canonical CLDR order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// Faces and emotion (`😀`, `❤️`, …).
    SmileysAndEmotion,
    /// People, body parts and gestures (`👋`, `🧑`, …).
    PeopleAndBody,
    /// Animals and nature (`🐶`, `🌸`, …).
    AnimalsAndNature,
    /// Food and drink (`🍇`, `☕`, …).
    FoodAndDrink,
    /// Travel and places (`🚗`, `🏔️`, …).
    TravelAndPlaces,
    /// Activities and events (`⚽`, `🎉`, …).
    Activities,
    /// Objects (`💡`, `📱`, …).
    Objects,
    /// Symbols (`❗`, `🔣`, …).
    Symbols,
    /// Flags (`🏁`, `🏳️‍🌈`, …).
    Flags,
}

impl Group {
    /// The nine groups, in canonical CLDR order.
    pub const ALL: [Self; 9] = [
        Self::SmileysAndEmotion,
        Self::PeopleAndBody,
        Self::AnimalsAndNature,
        Self::FoodAndDrink,
        Self::TravelAndPlaces,
        Self::Activities,
        Self::Objects,
        Self::Symbols,
        Self::Flags,
    ];

    /// A human-readable title for the group, as the reference picker labels its
    /// tabs.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::SmileysAndEmotion => "Smileys & Emotion",
            Self::PeopleAndBody => "People & Body",
            Self::AnimalsAndNature => "Animals & Nature",
            Self::FoodAndDrink => "Food & Drink",
            Self::TravelAndPlaces => "Travel & Places",
            Self::Activities => "Activities",
            Self::Objects => "Objects",
            Self::Symbols => "Symbols",
            Self::Flags => "Flags",
        }
    }

    /// Every emoji in this group, in CLDR order and default skin tone only.
    pub fn emojis(self) -> impl Iterator<Item = Emoji> {
        self.to_backing().emojis().map(Emoji::new)
    }

    /// Map a backing group into ours.
    pub(crate) const fn from_backing(group: emojis::Group) -> Self {
        match group {
            emojis::Group::SmileysAndEmotion => Self::SmileysAndEmotion,
            emojis::Group::PeopleAndBody => Self::PeopleAndBody,
            emojis::Group::AnimalsAndNature => Self::AnimalsAndNature,
            emojis::Group::FoodAndDrink => Self::FoodAndDrink,
            emojis::Group::TravelAndPlaces => Self::TravelAndPlaces,
            emojis::Group::Activities => Self::Activities,
            emojis::Group::Objects => Self::Objects,
            emojis::Group::Symbols => Self::Symbols,
            emojis::Group::Flags => Self::Flags,
        }
    }

    /// Map ours to the backing group.
    const fn to_backing(self) -> emojis::Group {
        match self {
            Self::SmileysAndEmotion => emojis::Group::SmileysAndEmotion,
            Self::PeopleAndBody => emojis::Group::PeopleAndBody,
            Self::AnimalsAndNature => emojis::Group::AnimalsAndNature,
            Self::FoodAndDrink => emojis::Group::FoodAndDrink,
            Self::TravelAndPlaces => emojis::Group::TravelAndPlaces,
            Self::Activities => emojis::Group::Activities,
            Self::Objects => emojis::Group::Objects,
            Self::Symbols => emojis::Group::Symbols,
            Self::Flags => emojis::Group::Flags,
        }
    }
}
