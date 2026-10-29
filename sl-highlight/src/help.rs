//! What an editor **offers** while a script is typed: completions, a call's
//! signature, and a symbol's description under the pointer.
//!
//! The shapes follow the Language Server Protocol's `CompletionItem`,
//! `SignatureInformation` and `Hover`, cut down to what an in-viewer editor
//! draws, so a language that later gets a real language server maps onto the
//! same widget. A language implements [`CodeAssist`]; the editor asks it at the
//! caret and draws the answers, knowing nothing of the language.
//!
//! The ranking of completions is the same for every language and lives here:
//! [`MatchTier`] and [`rank`].

use core::ops::Range;

use crate::Capture;

/// One thing an editor offers to complete the word at the caret with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What the list shows and what typing is matched against.
    pub label: String,
    /// What accepting puts in place of the word — usually the label, a whole
    /// handler header for an event.
    pub insert: String,
    /// How the label is coloured, as the editor colours the same word in the
    /// text; `None` for a word the editor leaves plain (a variable).
    pub capture: Option<Capture>,
    /// One line about it — a signature, a constant's type and value.
    pub detail: Option<String>,
    /// What the language's documentation says about it.
    pub documentation: Option<String>,
    /// Whether it is deprecated.
    pub deprecated: bool,
}

impl Completion {
    /// A completion of `label` that inserts itself, with nothing else said.
    #[must_use]
    pub fn word(label: impl Into<String>, capture: Option<Capture>) -> Self {
        let label = label.into();
        Self {
            insert: label.clone(),
            label,
            capture,
            detail: None,
            documentation: None,
            deprecated: false,
        }
    }
}

/// The completions for a caret, and the word they replace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completions {
    /// The bytes accepting a completion replaces: the whole word the caret is
    /// in or at the end of, or an empty range at the caret before any is typed.
    pub replace: Range<usize>,
    /// What is offered, best first.
    pub items: Vec<Completion>,
}

/// Something worth knowing about a call beyond its signature — a fact the
/// editor words in the resident's language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Note {
    /// The call puts the script to sleep for this many seconds.
    Sleep(f32),
    /// The symbol is deprecated.
    Deprecated,
    /// Only a grid god may use it.
    GodMode,
}

/// One parameter of a [`Signature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// Its bytes in the signature's [`label`](Signature::label) — what the
    /// editor emphasises while the caret is in its argument.
    pub label: Range<usize>,
    /// What the documentation says about it.
    pub documentation: Option<String>,
}

/// A function's signature, for the call the caret is in.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    /// The signature as one line: `integer llAbs(integer value)`.
    pub label: String,
    /// Its parameters, in order.
    pub parameters: Vec<Parameter>,
    /// The parameter the caret's argument is, if the call has that many.
    pub active: Option<usize>,
    /// What the documentation says about the function.
    pub documentation: Option<String>,
    /// What else is worth knowing.
    pub notes: Vec<Note>,
}

impl Signature {
    /// The active parameter's bytes in the label, if there is one.
    #[must_use]
    pub fn active_range(&self) -> Option<Range<usize>> {
        self.active
            .and_then(|active| self.parameters.get(active))
            .map(|parameter| parameter.label.clone())
    }
}

/// What the pointer is over.
#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    /// The bytes of the word described.
    pub range: Range<usize>,
    /// The word's declaration or signature, as one line.
    pub title: String,
    /// What the documentation says about it.
    pub documentation: Option<String>,
    /// What else is worth knowing.
    pub notes: Vec<Note>,
}

/// A language's help while its source is edited. Each answer is worked out from
/// the whole source and an offset into it, on the frame it is asked for, so —
/// like a highlighter — it has to be a lexer pass, not a parse.
pub trait CodeAssist: Send + Sync {
    /// What to offer at `caret` in `source`, or `None` where nothing is (inside
    /// a comment or a string, after a number).
    fn completions(&self, source: &str, caret: usize) -> Option<Completions>;

    /// The signature of the call whose parentheses `caret` is in.
    fn signature(&self, source: &str, caret: usize) -> Option<Signature>;

    /// What the word at byte `index` is.
    fn hover(&self, source: &str, index: usize) -> Option<Hover>;
}

/// How well a label matches what has been typed — the order a list is sorted
/// in, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchTier {
    /// The label starts with the typed text, case and all.
    Prefix,
    /// The label starts with the typed text, ignoring case.
    PrefixIgnoringCase,
    /// The label holds the typed text somewhere else, ignoring case — how
    /// `say` finds `llRegionSay`.
    Inside,
}

/// The fewest typed characters before a label that only *contains* them is
/// offered: one letter is inside nearly everything.
pub const MIN_INSIDE_MATCH: usize = 2;

/// How `label` matches `typed`, or `None` if it does not. Everything matches
/// an empty `typed`, as a prefix.
#[must_use]
pub fn match_tier(typed: &str, label: &str) -> Option<MatchTier> {
    if label.starts_with(typed) {
        return Some(MatchTier::Prefix);
    }
    let typed_lower = typed.to_lowercase();
    let label_lower = label.to_lowercase();
    if label_lower.starts_with(&typed_lower) {
        Some(MatchTier::PrefixIgnoringCase)
    } else if typed.chars().count() >= MIN_INSIDE_MATCH && label_lower.contains(&typed_lower) {
        Some(MatchTier::Inside)
    } else {
        None
    }
}

/// The `candidates` that match `typed`, best first: by [`MatchTier`], then
/// alphabetically ignoring case, with a label offered twice kept once. At most
/// `limit` of them.
#[must_use]
pub fn rank(
    typed: &str,
    candidates: impl IntoIterator<Item = Completion>,
    limit: usize,
) -> Vec<Completion> {
    let mut matched: Vec<(MatchTier, String, Completion)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            match_tier(typed, &candidate.label)
                .map(|tier| (tier, candidate.label.to_lowercase(), candidate))
        })
        .collect();
    matched.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.label.cmp(&b.2.label))
    });
    matched.dedup_by(|a, b| a.2.label == b.2.label);
    matched
        .into_iter()
        .take(limit)
        .map(|(_tier, _lower, completion)| completion)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Completion, MatchTier, Parameter, Signature, match_tier, rank};
    use pretty_assertions::assert_eq;

    /// The tiers: exact prefix, then prefix ignoring case, then inside.
    #[test]
    fn tiers() {
        assert_eq!(match_tier("llS", "llSay"), Some(MatchTier::Prefix));
        assert_eq!(
            match_tier("llsay", "llSay"),
            Some(MatchTier::PrefixIgnoringCase)
        );
        assert_eq!(match_tier("say", "llRegionSay"), Some(MatchTier::Inside));
        assert_eq!(match_tier("s", "llRegionSay"), None);
        assert_eq!(match_tier("", "anything"), Some(MatchTier::Prefix));
        assert_eq!(match_tier("x", "llSay"), None);
    }

    /// Ranked by tier, then alphabetically, duplicates dropped and capped.
    #[test]
    fn ranking() {
        let words = ["llRegionSay", "llSay", "llsayish", "llSetText", "llSay"];
        let ranked: Vec<String> = rank(
            "llSay",
            words.iter().map(|word| Completion::word(*word, None)),
            10,
        )
        .into_iter()
        .map(|completion| completion.label)
        .collect();
        assert_eq!(ranked, vec!["llSay", "llsayish"]);
        let ranked: Vec<String> = rank(
            "say",
            words.iter().map(|word| Completion::word(*word, None)),
            2,
        )
        .into_iter()
        .map(|completion| completion.label)
        .collect();
        assert_eq!(ranked, vec!["llRegionSay", "llSay"]);
    }

    /// The active parameter's range is read off the parameter list.
    #[test]
    fn the_active_parameter() {
        let signature = Signature {
            label: String::from("f(integer a, string b)"),
            parameters: vec![
                Parameter {
                    label: 2..11,
                    documentation: None,
                },
                Parameter {
                    label: 13..21,
                    documentation: None,
                },
            ],
            active: Some(1),
            documentation: None,
            notes: Vec::new(),
        };
        assert_eq!(signature.active_range(), Some(13..21));
        assert_eq!(
            Signature {
                active: Some(2),
                ..signature
            }
            .active_range(),
            None
        );
    }
}
