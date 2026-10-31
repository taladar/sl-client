//! How scripts talk: a script's listens, what a line it hears looks like, the
//! dialogs, instant messages and viewer prompts it hands its host, and who a
//! link message reaches (book chapter `gridspec/lsl.md`, § Chat, listens,
//! dialogs and link messages).
//!
//! The split with the host is positions against filters. *Which* scripts are
//! in earshot of a line is the world's question — ranges, the speaker's own
//! prim, a region border — and the host answers it; *whether* a script's
//! listens admit the line is the script's own, so the listens live here, in
//! the instance, and go with it at a state change and a reset. The host
//! offers each script in earshot the line through
//! [`Engine::hear`](crate::vm::Engine::hear).

use sl_types::lsl::Vector;

use crate::value::{NULL_KEY, is_uuid};
use crate::vm::flavour::EngineFlavour;
use crate::vm::host::CallerId;

/// How many listens one script may hold on Second Life: the 66th is the
/// run-time error `Too Many Listens` (`comms/listen-limit`, 2026-10-24).
pub const SECOND_LIFE_LISTENS: usize = 65;

/// How many listens one script may hold on OpenSim: the 65th, and every one
/// after it, is silently refused with a handle that is not positive
/// (`comms/listen-limit` on both engines).
pub const OPENSIM_LISTENS: usize = 64;

/// The one button of a text box: `llTextBox` is a dialog with it, and the
/// viewer shows a text field in its place.
pub const TEXT_BOX_BUTTON: &str = "!!llTextBox!!";

/// `llListen`'s filter: a channel, and a speaker name, a speaker key and a
/// message, each of which admits anything when left empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenFilter {
    /// The channel.
    pub channel: i32,
    /// The speaker's name exactly, or anyone's when empty.
    pub name: String,
    /// The speaker's key, or anyone when [`None`] — `NULL_KEY`, and a string
    /// that is no key, filter on nobody's key.
    pub speaker: Option<String>,
    /// The message exactly, or any message when empty.
    pub message: String,
}

impl ListenFilter {
    /// `llListen(channel, name, id, message)`'s filter.
    #[must_use]
    pub fn new(channel: i32, name: String, id: &str, message: String) -> Self {
        let speaker = (id != NULL_KEY && is_uuid(id)).then(|| id.to_ascii_lowercase());
        Self {
            channel,
            name,
            speaker,
            message,
        }
    }

    /// Whether the filter admits `heard`.
    #[must_use]
    pub fn admits(&self, heard: &Heard) -> bool {
        self.channel == heard.channel
            && (self.name.is_empty() || self.name == heard.name)
            && self
                .speaker
                .as_ref()
                .is_none_or(|speaker| speaker.eq_ignore_ascii_case(&heard.speaker))
            && (self.message.is_empty() || self.message == heard.message)
    }
}

/// A line a script is offered by its host: said on `channel` by `name`, whose
/// key is `speaker`. It becomes `listen(channel, name, speaker, message)`
/// once for every active listen of the script that admits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard {
    /// The channel it was said on.
    pub channel: i32,
    /// The speaker's name: an object's, or an avatar's as `llKey2Name` gives
    /// it (a dialog reply is said under the replying avatar's).
    pub name: String,
    /// The speaker's key.
    pub speaker: String,
    /// What was said.
    pub message: String,
}

/// One listen a script holds.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listen {
    /// What `llListen` answered.
    handle: i32,
    /// What it hears.
    filter: ListenFilter,
    /// Whether it is on (`llListenControl`).
    active: bool,
}

/// The listens of one script, in the order it opened them. A state change
/// and a reset drop them all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listens {
    /// The listens.
    listens: Vec<Listen>,
}

/// Why `llListen` opened nothing: the script holds as many as Second Life
/// allows, which is a run-time error there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyListens;

impl Listens {
    /// How many listens the script holds, on or off.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.listens.len()
    }

    /// Whether the script holds none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.listens.is_empty()
    }

    /// `llListen`: open a listen with `filter` and answer its handle, the
    /// lowest positive number the script does not use (OpenSim's
    /// `ListenerManager.GetNewHandle`).
    ///
    /// A filter the script already listens with answers that listen's handle
    /// and opens nothing, on both grids. Second Life takes it as the same
    /// filter only when every field is the same; OpenSim asks whether an
    /// active listen of the script would *hear* the new filter's fields as a
    /// line (`GetListeners(itemID, …)`), so a listen with an empty name also
    /// answers for a new one that names a speaker.
    ///
    /// # Errors
    ///
    /// [`TooManyListens`] when the script holds [`SECOND_LIFE_LISTENS`] on
    /// Second Life. On OpenSim, past [`OPENSIM_LISTENS`] the answer is `-1`
    /// and nothing is opened.
    pub fn open(
        &mut self,
        filter: ListenFilter,
        flavour: EngineFlavour,
    ) -> Result<i32, TooManyListens> {
        let same = |listen: &&Listen| {
            if flavour.is_opensim() {
                listen.active
                    && listen.filter.admits(&Heard {
                        channel: filter.channel,
                        name: filter.name.clone(),
                        speaker: filter
                            .speaker
                            .clone()
                            .unwrap_or_else(|| NULL_KEY.to_owned()),
                        message: filter.message.clone(),
                    })
            } else {
                listen.filter == filter
            }
        };
        if let Some(listen) = self.listens.iter().find(same) {
            return Ok(listen.handle);
        }
        let limit = if flavour.is_opensim() {
            OPENSIM_LISTENS
        } else {
            SECOND_LIFE_LISTENS
        };
        if self.listens.len() >= limit {
            return if flavour.is_opensim() {
                Ok(-1)
            } else {
                Err(TooManyListens)
            };
        }
        let handle = (1..=i32::MAX)
            .find(|candidate| {
                !self
                    .listens
                    .iter()
                    .any(|listen| listen.handle == *candidate)
            })
            .unwrap_or(-1);
        self.listens.push(Listen {
            handle,
            filter,
            active: true,
        });
        Ok(handle)
    }

    /// `llListenRemove`: close the listen `handle`; a handle the script does
    /// not hold is nothing, on both grids.
    pub fn remove(&mut self, handle: i32) {
        self.listens.retain(|listen| listen.handle != handle);
    }

    /// `llListenControl`: turn the listen `handle` off, or back on.
    pub fn control(&mut self, handle: i32, active: bool) {
        for listen in &mut self.listens {
            if listen.handle == handle {
                listen.active = active;
            }
        }
    }

    /// How many of the script's active listens admit `heard`: the number of
    /// `listen` events it raises. Two listens that both admit a line both
    /// raise one, as OpenSim's `DeliverMessage` queues one per listener (not
    /// measured on Second Life).
    #[must_use]
    pub fn admitting(&self, heard: &Heard) -> usize {
        self.listens
            .iter()
            .filter(|listen| listen.active && listen.filter.admits(heard))
            .count()
    }

    /// Drop every listen: the script changed state or was reset.
    pub fn clear(&mut self) {
        self.listens.clear();
    }
}

/// A dialog a script puts to an avatar — `llDialog`, or `llTextBox` with the
/// one button [`TEXT_BOX_BUTTON`] — already checked and cut as the region's
/// grid does. The host sends it as a `ScriptDialog`, under the object's name
/// and its owner's; the avatar's reply comes back as a line said on
/// [`Self::channel`] under the avatar's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    /// The avatar it is for: a key, never `NULL_KEY`.
    pub agent: String,
    /// The message.
    pub message: String,
    /// The buttons, in the script's order.
    pub buttons: Vec<String>,
    /// The channel the reply is said on.
    pub channel: i32,
}

/// `llInstantMessage`: an instant message from the object to an avatar. The
/// host sends it as Second Life does, an `IM_FROM_TASK` with the owner as
/// the sender and the prim's key as the message id, under the prim's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstantMessage {
    /// The avatar it is for: a key, never `NULL_KEY`.
    pub agent: String,
    /// The text, cut to what one message carries.
    pub text: String,
}

/// `llLoadURL`: offer an avatar a web page, with a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadUrl {
    /// The avatar it is for: a key, never `NULL_KEY`.
    pub agent: String,
    /// The message shown with the offer.
    pub message: String,
    /// The page.
    pub url: String,
}

/// `llMapDestination`: open an avatar's world map at a place.
#[derive(Debug, Clone, PartialEq)]
pub struct MapDestination {
    /// The avatar it is for: the one whose touch the script is handling, or
    /// the one wearing the object.
    pub agent: String,
    /// The region's name.
    pub region: String,
    /// Where in the region.
    pub position: Vector,
    /// Where the map is to look (unused by viewers, carried as sent).
    pub look_at: Vector,
}

/// One prim of a linkset, as link messages see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedPrim {
    /// Its link number: `0` for a prim linked to nothing, `1` for a
    /// linkset's root and `2…` for its children.
    pub link: i32,
    /// The scripts in it, each of which gets a link message to the prim.
    pub scripts: Vec<CallerId>,
}

/// The linkset a script is in, as its host describes it for
/// `llMessageLinked`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Links {
    /// The link number of the caller's own prim: the sender number every
    /// link message it sends carries.
    pub this: i32,
    /// Every prim of the linkset in link order, the root first.
    pub prims: Vec<LinkedPrim>,
}

/// `LINK_ROOT`, which is also the link number 1.
const LINK_ROOT: i32 = 1;
/// `LINK_SET`.
const LINK_SET: i32 = -1;
/// `LINK_ALL_OTHERS`.
const LINK_ALL_OTHERS: i32 = -2;
/// `LINK_ALL_CHILDREN`.
const LINK_ALL_CHILDREN: i32 = -3;
/// `LINK_THIS`.
const LINK_THIS: i32 = -4;

impl Links {
    /// The scripts a link message to `target` reaches, prim by prim in link
    /// order: `LINK_SET` every prim, `LINK_ALL_OTHERS` all but the sender's,
    /// `LINK_ALL_CHILDREN` all but the root, `LINK_THIS` the sender's, and a
    /// number the prim of that link number — the sender's own scripts,
    /// itself included, whenever its prim is among them. A prim linked to
    /// nothing is link 0, so on Second Life `LINK_ROOT` (1) reaches nobody;
    /// OpenSim takes it for the root whatever its number
    /// (`comms/link-messages`).
    #[must_use]
    pub fn reached(&self, target: i32, flavour: EngineFlavour) -> Vec<CallerId> {
        let root = self.prims.first().map(|prim| prim.link);
        let reaches = |prim: &LinkedPrim| match target {
            LINK_SET => true,
            LINK_ALL_OTHERS => prim.link != self.this,
            LINK_ALL_CHILDREN => Some(prim.link) != root,
            LINK_THIS => prim.link == self.this,
            LINK_ROOT if flavour.is_opensim() => Some(prim.link) == root,
            number if number >= 0 => prim.link == number,
            _ => false,
        };
        self.prims
            .iter()
            .filter(|prim| reaches(prim))
            .flat_map(|prim| prim.scripts.iter().copied())
            .collect()
    }
}

/// A link message a script sent, waiting for the end of its slice to be
/// delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMessage {
    /// `llMessageLinked`'s link: a number or a `LINK_*` constant.
    pub target: i32,
    /// The number.
    pub num: i32,
    /// The string, whole.
    pub text: String,
    /// The key, whole, whether or not it is one.
    pub id: String,
}

/// `text` cut to at most `bytes` bytes of UTF-8, at a character's end.
#[must_use]
pub(crate) fn cut_bytes(text: &str, bytes: usize) -> &str {
    if text.len() <= bytes {
        return text;
    }
    let end = (0..=bytes)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text.get(..end).unwrap_or_default()
}

/// Whether `key` names somebody: a key, and not `NULL_KEY`.
#[must_use]
pub(crate) fn is_somebody(key: &str) -> bool {
    key != NULL_KEY && is_uuid(key)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A line on `channel` from a speaker called `name` with key `speaker`.
    fn line(channel: i32, name: &str, speaker: &str, message: &str) -> Heard {
        Heard {
            channel,
            name: name.to_owned(),
            speaker: speaker.to_owned(),
            message: message.to_owned(),
        }
    }

    /// A speaker's key.
    const KEY: &str = "0f16c0e1-384e-4b5f-b7ce-886dda3bce41";

    #[test]
    fn an_empty_field_admits_anything_and_a_set_one_only_itself() {
        let any = ListenFilter::new(5, String::new(), NULL_KEY, String::new());
        assert!(any.admits(&line(5, "Bob", KEY, "hi")));
        assert!(!any.admits(&line(6, "Bob", KEY, "hi")));
        let named = ListenFilter::new(5, "Bob".to_owned(), "", String::new());
        assert!(named.admits(&line(5, "Bob", KEY, "hi")));
        assert!(!named.admits(&line(5, "Bobby", KEY, "hi")));
        let keyed = ListenFilter::new(5, String::new(), &KEY.to_ascii_uppercase(), String::new());
        assert!(keyed.admits(&line(5, "Bob", KEY, "hi")));
        assert!(!keyed.admits(&line(5, "Bob", NULL_KEY, "hi")));
        let not_a_key = ListenFilter::new(5, String::new(), "not a key", String::new());
        assert!(not_a_key.admits(&line(5, "Bob", KEY, "hi")));
        let said = ListenFilter::new(9, String::new(), "", "match".to_owned());
        assert!(said.admits(&line(9, "Bob", KEY, "match")));
        assert!(!said.admits(&line(9, "Bob", KEY, "nomatch")));
    }

    #[test]
    fn handles_are_the_lowest_free_and_a_repeat_filter_is_the_same_listen() {
        for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
            let mut listens = Listens::default();
            let filter = |channel| ListenFilter::new(channel, String::new(), "", String::new());
            assert_eq!(listens.open(filter(1), flavour), Ok(1));
            assert_eq!(listens.open(filter(2), flavour), Ok(2));
            assert_eq!(listens.open(filter(1), flavour), Ok(1));
            listens.remove(1);
            listens.remove(-12_345);
            assert_eq!(listens.open(filter(3), flavour), Ok(1));
            assert_eq!(listens.len(), 2);
        }
    }

    #[test]
    fn opensim_takes_a_listen_that_would_hear_the_new_filter_for_it() {
        let open = |flavour| {
            let mut listens = Listens::default();
            let any = listens.open(
                ListenFilter::new(5, String::new(), "", String::new()),
                flavour,
            );
            let bob = listens.open(
                ListenFilter::new(5, "Bob".to_owned(), "", String::new()),
                flavour,
            );
            (any, bob)
        };
        assert_eq!(open(EngineFlavour::SecondLife), (Ok(1), Ok(2)));
        assert_eq!(open(EngineFlavour::YEngine), (Ok(1), Ok(1)));
    }

    #[test]
    fn the_limit_is_an_error_on_second_life_and_a_refusal_on_opensim() {
        let fill = |flavour| {
            let mut listens = Listens::default();
            (1..=70)
                .map(|channel| {
                    listens.open(
                        ListenFilter::new(channel, String::new(), "", String::new()),
                        flavour,
                    )
                })
                .collect::<Vec<_>>()
        };
        let second_life = fill(EngineFlavour::SecondLife);
        assert_eq!(second_life.get(64), Some(&Ok(65)));
        assert_eq!(second_life.get(65), Some(&Err(TooManyListens)));
        let opensim = fill(EngineFlavour::XEngine);
        assert_eq!(opensim.get(63), Some(&Ok(64)));
        assert_eq!(opensim.get(64), Some(&Ok(-1)));
        assert_eq!(opensim.get(69), Some(&Ok(-1)));
    }

    #[test]
    fn a_listen_turned_off_hears_nothing_until_turned_on() {
        let mut listens = Listens::default();
        let filter = ListenFilter::new(5, String::new(), "", String::new());
        let handle = listens.open(filter, EngineFlavour::SecondLife);
        assert_eq!(handle, Ok(1));
        let heard = line(5, "Bob", KEY, "hi");
        assert_eq!(listens.admitting(&heard), 1);
        listens.control(1, false);
        assert_eq!(listens.admitting(&heard), 0);
        listens.control(1, true);
        assert_eq!(listens.admitting(&heard), 1);
    }

    /// A lone prim with scripts 1 and 2.
    fn lone() -> Links {
        Links {
            this: 0,
            prims: vec![LinkedPrim {
                link: 0,
                scripts: vec![CallerId(1), CallerId(2)],
            }],
        }
    }

    /// A linkset of three prims with a script in each, the caller in the
    /// second.
    fn linkset() -> Links {
        Links {
            this: 2,
            prims: (1..=3)
                .map(|link| LinkedPrim {
                    link,
                    scripts: vec![CallerId(u64::try_from(link).unwrap_or_default())],
                })
                .collect(),
        }
    }

    #[test]
    fn link_targets_in_a_lone_prim_are_as_the_grids_answered() {
        let both = vec![CallerId(1), CallerId(2)];
        for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
            for target in [LINK_THIS, LINK_SET, 0] {
                assert_eq!(lone().reached(target, flavour), both, "{target}");
            }
            for target in [LINK_ALL_OTHERS, LINK_ALL_CHILDREN, 2, -5] {
                assert_eq!(lone().reached(target, flavour), Vec::new(), "{target}");
            }
        }
        assert_eq!(
            lone().reached(LINK_ROOT, EngineFlavour::SecondLife),
            Vec::new()
        );
        assert_eq!(lone().reached(LINK_ROOT, EngineFlavour::XEngine), both);
    }

    #[test]
    fn link_targets_in_a_linkset() {
        let flavour = EngineFlavour::SecondLife;
        let ids = |ids: &[u64]| ids.iter().copied().map(CallerId).collect::<Vec<_>>();
        assert_eq!(linkset().reached(LINK_SET, flavour), ids(&[1, 2, 3]));
        assert_eq!(linkset().reached(LINK_ALL_OTHERS, flavour), ids(&[1, 3]));
        assert_eq!(linkset().reached(LINK_ALL_CHILDREN, flavour), ids(&[2, 3]));
        assert_eq!(linkset().reached(LINK_THIS, flavour), ids(&[2]));
        assert_eq!(linkset().reached(LINK_ROOT, flavour), ids(&[1]));
        assert_eq!(linkset().reached(3, flavour), ids(&[3]));
        assert_eq!(linkset().reached(0, flavour), Vec::new());
    }

    #[test]
    fn a_cut_ends_at_a_character() {
        assert_eq!(cut_bytes("abc", 5), "abc");
        assert_eq!(cut_bytes("abc", 2), "ab");
        assert_eq!(cut_bytes("aé", 2), "a");
        assert_eq!(cut_bytes("aé", 3), "aé");
    }
}
