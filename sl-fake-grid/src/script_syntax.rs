//! The script syntax a region advertises and serves: the `LSLSyntaxId` (and on
//! Second Life the `LSLSyntaxVersion`) in `SimulatorFeatures`, and the
//! document the `LSLSyntax` capability answers with.
//!
//! A viewer fetches that document on every login to a region whose id it has
//! not cached, and colours and completes its script editor from it. Both live
//! grids serve one (`simulator-features`, 2026-10-07): Second Life a document
//! per simulator version, OpenSim the one in its `bin/ScriptSyntax.xml`.
//!
//! # Which document a flavour serves
//!
//! Each flavour serves **the live grid's own document**, byte for byte
//! ([`LslSyntaxDocument::second_life`], [`LslSyntaxDocument::open_sim`]), not
//! one rendered from `sl-lsl-runtime`'s descriptor table. The fake grid exists
//! to fail a viewer the way a real grid would, and the editor a viewer builds
//! from a served document is part of what it does with one: the two grids'
//! documents differ in shape (OpenSim's carries no energy or sleep, its
//! tooltips lose their line breaks, it adds the `os*` functions) and in what
//! they leave out — OpenSim's lists every `os*` function whatever the region's
//! `[OSSL]` configuration denies, and none of the `mod*` functions or
//! `osListenRegex`, which compile and run there (`gridspec-lsl-ossl`,
//! 2026-10-26). A rendered document would hide all of that. For Second Life the
//! two would barely differ anyway: the runtime's table *is* the document Second
//! Life serves, generated from the same vendored file.
//!
//! A test that wants any other document — one rendered from a table of its
//! own, the engine's included — builds it with
//! [`LslSyntaxDocument::from_table`], and the two cases a viewer must survive
//! are knobs too: a grid that advertises nothing ([`LslSyntaxPolicy::Withhold`],
//! what an OpenSim region without the file does) and a document at a schema
//! version the decoder refuses ([`LslSyntaxDocument::at_schema_version`]).
//!
//! # The id follows the content
//!
//! A viewer caches a document by its id, so the id must change whenever the
//! content does. The two vendored documents carry the id their grid advertised
//! for them, which changes when the file is refreshed; a document built here is
//! named by a hash of its text.

use std::sync::{Arc, LazyLock};

use sl_wire::LslSyntax;
use uuid::Uuid;

/// Linden Lab's `LSLSyntax` document as Second Life served it: aditi, region
/// Mauve, simulator `2026-07-24.30110090421`, 2026-10-26. The copy
/// `sl-lsl-runtime` builds its library table from, read from there so the
/// table and what the fake grid serves cannot drift apart (provenance in
/// `sl-lsl-runtime/README.md`).
const SECOND_LIFE_DOCUMENT: &str = include_str!("../../sl-lsl-runtime/keywords_lsl_default.xml");

/// The `LSLSyntaxId` Second Life advertised beside [`SECOND_LIFE_DOCUMENT`].
const SECOND_LIFE_ID: Uuid = Uuid::from_u128(0xdc54_9834_e9a2_9600_94ce_019d_78e5_2302);

/// The `LSLSyntaxVersion` Second Life advertised beside
/// [`SECOND_LIFE_DOCUMENT`] (`records/aditi/simulator-features.toml`).
const SECOND_LIFE_VERSION: &str = "0.6.12";

/// OpenSim's `bin/ScriptSyntax.xml`, vendored verbatim (BSD, see
/// `lsl-syntax/README.md`): the id on its first line, the document after it.
const OPEN_SIM_FILE: &str = include_str!("../lsl-syntax/ScriptSyntax.xml");

/// [`LslSyntaxDocument::second_life`], shared by every session.
static SECOND_LIFE: LazyLock<LslSyntaxDocument> = LazyLock::new(|| LslSyntaxDocument {
    id: SECOND_LIFE_ID,
    version: Some(SECOND_LIFE_VERSION.to_owned()),
    text: Arc::from(SECOND_LIFE_DOCUMENT),
});

/// [`LslSyntaxDocument::open_sim`], shared by every session; [`None`] when the
/// vendored file does not open with an id, which a test catches.
static OPEN_SIM: LazyLock<Option<LslSyntaxDocument>> = LazyLock::new(|| {
    let (id, text) = open_sim_served(OPEN_SIM_FILE)?;
    Some(LslSyntaxDocument {
        id,
        version: None,
        text: Arc::from(text),
    })
});

/// What OpenSim serves for its `ScriptSyntax.xml`
/// (`SimulatorFeaturesModule.ReadScriptSyntax`): the id on the first line, and
/// every later line trimmed of spaces, tabs and line ends and joined without a
/// separator, blank lines and lines that open a comment dropped. [`None`] when
/// the first line is no id, in which case OpenSim advertises and serves
/// nothing.
fn open_sim_served(file: &str) -> Option<(Uuid, String)> {
    let trim: &[char] = &[' ', '\t', '\n', '\r'];
    let mut lines = file.lines();
    let id = Uuid::parse_str(lines.next()?.trim_matches(trim)).ok()?;
    let text = lines
        .map(|line| line.trim_matches(trim))
        .filter(|line| !line.is_empty() && !line.starts_with("<!--"))
        .collect();
    Some((id, text))
}

/// The id of a document built here: a hash of its text, so a changed document
/// is a new id and a viewer's cached copy of the old one is not reused.
fn content_id(text: &str) -> Uuid {
    uuid::Builder::from_md5_bytes(md5::compute(text.as_bytes()).0).into_uuid()
}

/// One `LSLSyntax` document a region serves, with the id it advertises for it
/// and, on Second Life, the version string beside the id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LslSyntaxDocument {
    /// The `LSLSyntaxId` advertised for the document.
    id: Uuid,
    /// The `LSLSyntaxVersion` advertised beside the id, which only Second
    /// Life sends.
    version: Option<String>,
    /// The document, served byte for byte.
    text: Arc<str>,
}

impl LslSyntaxDocument {
    /// The document Second Life serves, with the id and version it advertised
    /// for it.
    #[must_use]
    pub fn second_life() -> Self {
        SECOND_LIFE.clone()
    }

    /// The document a stock OpenSim region serves: its `ScriptSyntax.xml`,
    /// with the id on its first line and no version.
    ///
    /// # Panics
    ///
    /// Never for the vendored file, whose first line is an id; a test holds
    /// it to that.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "the vendored file is a constant, and a test proves it opens with an id"
    )]
    pub fn open_sim() -> Self {
        OPEN_SIM
            .clone()
            .expect("the vendored ScriptSyntax.xml opens with its id")
    }

    /// A document rendered from `syntax`, named by a hash of the rendering and
    /// advertised with no version.
    #[must_use]
    pub fn from_table(syntax: &LslSyntax) -> Self {
        let text = sl_wire::build_lsl_syntax_document(syntax);
        Self {
            id: content_id(&text),
            version: None,
            text: Arc::from(text),
        }
    }

    /// An empty document declaring schema version `version`. Any version but
    /// the one the viewers implement (2) is one a viewer must refuse rather
    /// than parse against a schema it does not know, and survive.
    #[must_use]
    pub fn at_schema_version(version: i32) -> Self {
        let text = format!(
            "<llsd><map><key>llsd-lsl-syntax-version</key><integer>{version}</integer>\
             </map></llsd>"
        );
        Self {
            id: content_id(&text),
            version: None,
            text: Arc::from(text),
        }
    }

    /// The `LSLSyntaxId` advertised for the document.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }

    /// The `LSLSyntaxVersion` advertised beside the id, if any.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The document as served.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The document as served, shared rather than copied, for a session to
    /// serve.
    #[must_use]
    pub(crate) fn shared_text(&self) -> Arc<str> {
        Arc::clone(&self.text)
    }
}

/// What a grid's regions advertise and serve as their script syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LslSyntaxPolicy {
    /// Advertise the document's id (and version) in `SimulatorFeatures` and
    /// serve it from the `LSLSyntax` capability.
    Serve(LslSyntaxDocument),
    /// Advertise no id and grant no `LSLSyntax` capability — an OpenSim region
    /// without its `ScriptSyntax.xml`, or with `[SimulatorFeatures]
    /// ScriptSyntax = false`. A viewer falls back to the syntax it ships.
    Withhold,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::{LslSyntaxDocument, OPEN_SIM, OPEN_SIM_FILE, open_sim_served};

    /// Decodes a served document the way a viewer does.
    fn decode(document: &LslSyntaxDocument) -> Result<sl_wire::LslSyntax, String> {
        let llsd = sl_wire::parse_llsd_xml(document.text()).map_err(|error| error.to_string())?;
        sl_wire::parse_lsl_syntax(&llsd).map_err(|error| error.to_string())
    }

    #[test]
    fn the_vendored_open_sim_file_opens_with_its_id() {
        assert!(OPEN_SIM.is_some());
        assert_eq!(
            LslSyntaxDocument::open_sim().id().to_string(),
            OPEN_SIM_FILE.lines().next().unwrap_or_default().trim()
        );
    }

    /// OpenSim's transform: trimmed lines joined, comments and blank lines
    /// dropped, and a file whose first line is no id serves nothing.
    #[test]
    fn open_sim_serves_its_file_trimmed_and_joined() {
        let file = "4b833b57-d52b-5503-85a4-76754ac3b8ff \r\n<llsd>\n  <!-- a note -->\n\n\t<map>\
                    </map>  \n</llsd>\n";
        assert_eq!(
            open_sim_served(file).map(|(id, text)| (id.to_string(), text)),
            Some((
                "4b833b57-d52b-5503-85a4-76754ac3b8ff".to_owned(),
                "<llsd><map></map></llsd>".to_owned()
            ))
        );
        assert_eq!(open_sim_served("<llsd></llsd>\n"), None);
    }

    /// Both flavours' documents decode, each with the functions only its grid
    /// lists, and Second Life's alone carries a version.
    #[test]
    fn each_flavour_serves_a_document_a_viewer_decodes() -> Result<(), String> {
        let second_life = decode(&LslSyntaxDocument::second_life())?;
        let open_sim = decode(&LslSyntaxDocument::open_sim())?;
        assert!(second_life.function("llLinksetDataWrite").is_some());
        assert!(second_life.function("osGetSimulatorVersion").is_none());
        assert!(open_sim.function("osGetSimulatorVersion").is_some());
        assert!(open_sim.function("llSay").is_some());
        assert_eq!(LslSyntaxDocument::second_life().version(), Some("0.6.12"));
        assert_eq!(LslSyntaxDocument::open_sim().version(), None);
        Ok(())
    }

    /// A table rendered and decoded again is the table, and its id changes
    /// with its content.
    #[test]
    fn a_table_round_trips_under_an_id_that_follows_it() -> Result<(), String> {
        let mut syntax = decode(&LslSyntaxDocument::open_sim())?;
        let document = LslSyntaxDocument::from_table(&syntax);
        assert_eq!(decode(&document)?, syntax);
        assert_eq!(LslSyntaxDocument::from_table(&syntax).id(), document.id());
        let _removed = syntax.functions.remove("llSay");
        assert_ne!(LslSyntaxDocument::from_table(&syntax).id(), document.id());
        Ok(())
    }

    #[test]
    fn a_document_at_another_schema_version_is_refused() {
        let error = decode(&LslSyntaxDocument::at_schema_version(3)).err();
        assert!(error.is_some_and(|error| error.contains('3')), "accepted");
    }
}
