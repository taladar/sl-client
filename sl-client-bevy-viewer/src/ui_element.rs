//! The **UI element registry** (`viewer-ui-test-harness`): the one list of
//! things this viewer's UI is built from, shared by the gallery a human looks at
//! ([`crate::gallery`]) and the checks a machine runs ([`crate::ui_test`]).
//!
//! # Why a registry, rather than each test spawning what it wants
//!
//! Because that is what makes the test suite **compound**. A human eyeballs a
//! new panel and spots a bug; the fix is a *check*, and the moment that check
//! exists it runs against every element in this list — not just the panel the
//! bug was found in, and not just today. Conversely a new element registered
//! here inherits the entire accumulated suite for free, across every font,
//! script, direction, scale and translation length in the matrix. Neither half
//! of that works if elements are spawned ad hoc inside individual tests: checks
//! and elements have to meet in a list, or every pairing is a thing somebody has
//! to remember to write.
//!
//! **A new panel or widget belongs in [`ELEMENTS`].** That is the whole
//! obligation, and it is what buys the element every check that exists now and
//! every check added later.
//!
//! # The rule this registry enforces: construction without wiring
//!
//! An element must be **constructible without its actions**. In the gallery a
//! button must be clickable without firing what it does in the live viewer — no
//! teleport, no object edit, no L$ spent — and in a test it must be drivable
//! with nothing behind it at all.
//!
//! So an element never calls into a live session. It **emits a [`UiAction`]**
//! and someone else decides what that means:
//!
//! | Consumer | What a click does |
//! | --- | --- |
//! | the viewer | a real handler reads the [`UiAction`] and acts on the session |
//! | the gallery | nothing reads it — the click is inert by construction |
//! | the harness | the test reads it and asserts the element *would* have acted |
//!
//! This is why the rule is not a burden: routing actions as messages is what
//! makes an element's behaviour **assertable** at all. A button wired directly
//! to a `Session` cannot be tested without a grid; a button that emits a
//! [`UiAction`] is tested by reading a queue.
//!
//! A panel that can only be spawned by reaching for a live `Session` is a panel
//! that can never be tested, and retrofitting the separation later is exactly
//! the late rework the scaffold exists to prevent.
//!
//! # What is in here, and what is not
//!
//! The **vocabulary**, not the viewer's finished panels — because at the time of
//! writing there are none: the ~40 generic primitives come from
//! `bevy_ui_widgets`, and the ~100 viewer-domain composites (trees, chiclets,
//! texture pickers, the net map) are each their own roadmap task, unwritten. The
//! elements here are the patterns those composites will be built out of, seeded
//! so that the mechanism is load-bearing from the first one rather than
//! retrofitted at the hundredth.
//!
//! The `F4` / `F5` demo panels (`crate::ui_text`, `crate::ui`) are deliberately
//! *not* registered: they are hand-driven demonstrations of the scaffold whose
//! text is written every frame from resources, so they cannot show a
//! pseudolocalised or CJK string and would test the demo rather than the UI.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use bevy::ui_widgets::{Activate, Button};

use crate::ui::{LogicalPadding, LogicalRect, column, row};
use crate::ui_font::UiFont;
use crate::ui_pseudoloc::pseudolocalise;

/// Something an element would do, had anything been listening.
///
/// The whole of an element's outward wiring. See the [module
/// documentation](self): the viewer routes these to real handlers, the gallery
/// routes them nowhere, and a test reads them to assert what a click meant.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiAction {
    /// The [`UiElement::id`] of the element that emitted it.
    pub(crate) element: &'static str,
    /// Which of that element's actions fired.
    pub(crate) action: &'static str,
}

/// A sample of one writing system, for the matrix.
///
/// Two lengths because they fail differently: a short label overflows its own
/// box, while a long one forces the wrap that
/// `viewer-text-node-padding-measure` gets wrong — and a matrix carrying only
/// one of them misses half the bugs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScriptSample {
    /// The script's name, for a failure message and the gallery's label.
    pub(crate) name: &'static str,
    /// A label-length sample.
    pub(crate) short: &'static str,
    /// A prose-length sample, long enough to wrap in a panel.
    pub(crate) long: &'static str,
}

/// The scripts every element is checked in.
///
/// Chosen for the distinct ways each breaks a layout, not for coverage of the
/// world's languages: Latin is the baseline every string was measured in; CJK is
/// short but tall and needs a fallback face; Cyrillic is alphabetic but
/// non-ASCII; Arabic and Hebrew are RTL *and* shaped; Devanagari reorders and
/// stacks; emoji are colour bitmaps of an entirely different metric; and bidi is
/// the mix, which is where the ordering bugs live and which no single-script
/// sample can reach.
pub(crate) const SCRIPTS: &[ScriptSample] = &[
    ScriptSample {
        name: "Latin",
        short: "Save changes",
        long: "The quick brown fox jumps over the lazy dog, and then does it again to be sure \
               the line has to wrap somewhere sensible.",
    },
    ScriptSample {
        name: "CJK",
        short: "変更を保存",
        long: "この文章は、折り返しが正しく行われるかどうかを確認するために十分な長さを持って\
               います。日本語の文字は英語よりも背が高く、行の高さが変わります。",
    },
    ScriptSample {
        name: "Cyrillic",
        short: "Сохранить",
        long: "Съешь же ещё этих мягких французских булок да выпей чаю, и убедись, что строка \
               переносится там, где нужно.",
    },
    ScriptSample {
        name: "Arabic",
        short: "حفظ التغييرات",
        long: "هذا نص طويل بما فيه الكفاية للتأكد من أن الأسطر تلتف بشكل صحيح، وأن الاتجاه من \
               اليمين إلى اليسار يعمل كما ينبغي.",
    },
    ScriptSample {
        name: "Hebrew",
        short: "שמור שינויים",
        long: "זהו טקסט ארוך מספיק כדי לוודא שהשורות נשברות במקום הנכון, ושהכיוון מימין לשמאל \
               פועל כראוי.",
    },
    ScriptSample {
        name: "Devanagari",
        short: "परिवर्तन सहेजें",
        long: "यह पाठ इतना लंबा है कि यह सुनिश्चित किया जा सके कि पंक्तियाँ सही जगह पर टूटती हैं और \
               अक्षर सही ढंग से जुड़ते हैं।",
    },
    ScriptSample {
        name: "Emoji",
        short: "💾 🎉 ✨",
        long: "🌍 🚀 ✨ 🎉 💾 🔥 🌈 🐍 🦀 🎨 📦 🔧 🧪 🛠 🌟 🎯 🧭 🗺 🧩 🔍 🧵 🪄 🎲 🧊 \
               🌊 🍕 🐙 🦑 🪐 ☄️",
    },
    ScriptSample {
        name: "Bidi",
        short: "Save שמור حفظ",
        long: "A line that mixes English with עברית and العربية in one paragraph, so the \
               bidirectional algorithm has to resolve the runs and the wrap has to fall \
               somewhere legal.",
    },
];

/// How one matrix cell transforms an element's own strings.
///
/// An element is authored with English literals, exactly as a real panel is. The
/// cell decides what those literals become — which is a faithful model of what
/// `viewer-i18n-fluent-scaffold` will do for real, where the literal becomes a
/// Fluent key and the bundle decides the rest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SampleText {
    /// The element's own English literals, untouched — the baseline.
    #[default]
    Native,
    /// Pseudolocalised: accented, ~40% longer, fenced. Stands in for a real
    /// translation before one exists ([`crate::ui_pseudoloc`]).
    Pseudo,
    /// Replaced with a sample in another writing system.
    Script(&'static ScriptSample),
}

impl SampleText {
    /// The length at or above which a string is treated as prose rather than a
    /// label, and gets a [`ScriptSample::long`] rather than a
    /// [`ScriptSample::short`].
    ///
    /// A heuristic, and a deliberately crude one: the point is only that a
    /// button keeps a button-sized string and a paragraph keeps a
    /// paragraph-sized one, so that a script swap tests the same *layout
    /// problem* the original string posed rather than a different one.
    const PROSE_CHARS: usize = 40;

    /// Apply this cell's transform to one of an element's strings.
    pub(crate) fn apply(self, original: &str) -> String {
        match self {
            Self::Native => original.to_owned(),
            Self::Pseudo => pseudolocalise(original),
            Self::Script(sample) => {
                if original.chars().count() >= Self::PROSE_CHARS {
                    sample.long.to_owned()
                } else {
                    sample.short.to_owned()
                }
            }
        }
    }

    /// This cell's name, for a failure message and the gallery's label.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Native => "Native",
            Self::Pseudo => "Pseudo",
            Self::Script(sample) => sample.name,
        }
    }
}

/// What an element is spawned with: everything a cell varies that the element
/// itself must not hard-code.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ElementCx {
    /// How this cell transforms the element's strings.
    pub(crate) text: SampleText,
    /// The font size the element's text is set at, in logical pixels. A separate
    /// axis from the string: a panel that survives a long translation can still
    /// break when the user turns the UI font up.
    pub(crate) font_size: f32,
}

/// The font size an element uses when a cell does not say otherwise.
const DEFAULT_FONT_SIZE: f32 = 15.0;

impl ElementCx {
    /// A context at the resting configuration: native strings at the default
    /// size.
    pub(crate) const fn new() -> Self {
        Self {
            text: SampleText::Native,
            font_size: DEFAULT_FONT_SIZE,
        }
    }

    /// Transform one of the element's own strings for this cell.
    pub(crate) fn text(self, original: &str) -> String {
        self.text.apply(original)
    }

    /// This cell's font, at this cell's size.
    pub(crate) fn font(self, role: UiFont) -> TextFont {
        role.at(self.font_size)
    }
}

/// A declared alignment: every node carrying the same [`group`](Self::group)
/// must share the named [`edge`](Self::edge).
///
/// **Why this is a declaration and not an invariant.** Nothing in a layout tree
/// says whether two labels *ought* to line up, so a harness cannot infer it, and
/// guessing would bury a real finding in noise. The element author knows, states
/// it once here, and the harness then holds the element to it in **every** cell
/// of the matrix.
///
/// That is what makes it worth the ceremony. Edges that agree in English agree
/// by accident — the strings happened to be the same length. The failure is a
/// language where they are not: a label column that is a tidy rule in English
/// and a ragged edge in German, which is invisible to the author and obvious to
/// the reader. Declared once, [`crate::ui_test::alignment_violations`] checks it
/// against every script, every translation length and every UI scale.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AlignmentGroup {
    /// The group's name — nodes sharing it must line up. Also what a failure
    /// message names.
    pub(crate) group: &'static str,
    /// Which edge of those nodes must agree.
    pub(crate) edge: AlignEdge,
}

/// Declares that the text in this subtree **may legitimately be clipped
/// mid-glyph**, so [`crate::ui_test::clipping_violations`] must not report it.
///
/// The escape hatch for the one universal check that is not, on inspection,
/// universal. A label sliced in half is always a bug — but plenty of correct
/// widgets slice text on purpose:
///
/// - a **single-line field** scrolls its content horizontally as you type past
///   the end, so the text is half-clipped for as long as it is too long;
/// - a **multi-line editor that does not wrap** clips long lines by design,
///   because breaking a URL or a script line would be worse;
/// - **chat** and any other surface with text nobody measured in advance.
///
/// Without this the check would fire on every one of those, and a check that
/// cries wolf is a check somebody eventually deletes — taking the real findings
/// with it. So the rule inverts: clipping text is a **declaration**, not a
/// default. Every exception is greppable, carries a [`reason`](Self::reason),
/// and can be argued with; anything that has not claimed the exception is held
/// to the strict rule.
///
/// This is *not* needed for a node under an `Overflow::Scroll` ancestor: that is
/// already an explicit structural statement that content is clipped and reached
/// by scrolling, and the check reads it directly.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextMayClip {
    /// Why this subtree is allowed to slice its text. Not decoration: it is what
    /// makes an audit of the exceptions possible, and what a reviewer argues
    /// with when the exception is really a bug wearing a declaration.
    pub(crate) reason: &'static str,
}

/// A declared **radial** placement: this node must lie in the named direction
/// from its group's [`RadialCentre`], to within [`tolerance`](Self::tolerance).
///
/// **Why the universal checks cannot cover this.** Every other check in
/// [`crate::ui_test`] reasons about boxes — content inside its box, a box inside
/// its parent, text not sliced. That vocabulary is exhausted by a layout made of
/// rectangles, and a radial menu is not: its slices are *angular sectors drawn by
/// a shader*, with no nodes of their own for a harness to measure. Every box in a
/// pie can be perfectly legal while the widget is completely broken.
///
/// And it is a real failure, not a hypothetical one. `crate::pie_menu` picks the
/// slice under the pointer from the **angle** to the ring's centre, and places its
/// labels on wedges at a radius. Get the placement arithmetic wrong — or let a
/// label grow enough that its box centre shifts — and the *angle* from the centre
/// to the north-east label creeps toward due east, and pointing at a label that
/// says one thing selects another. Nothing about the layout looks wrong. Every box
/// is inside its parent. The menu simply lies, in one language, at one font size.
///
/// So the element declares the geometry it means, in the terms it actually means
/// it — a direction and a tolerance, not a rectangle — and the harness holds it
/// to that in every cell of the matrix, exactly as [`AlignmentGroup`] does for
/// columns that must stay straight.
///
/// Like [`AlignmentGroup`] and [`TextMayClip`], this is **declared** by an element
/// and read only by [`crate::ui_test`]. That is not a smell: a declaration exists
/// to be checked, the checker is the harness, and the gallery deliberately does
/// not check anything (see [`crate::gallery`] — it answers "does this look right",
/// which is the question a machine cannot).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct RadialPlacement {
    /// The group's name, tying this node to its [`RadialCentre`].
    pub(crate) group: &'static str,
    /// The direction this node must lie in, in radians counter-clockwise from
    /// due east, in a **y-up** frame (`crate::pie_menu::ui_offset` converts).
    pub(crate) angle: f32,
    /// How far the node's actual direction may stray, in radians.
    ///
    /// For a pie this is **half a slice**: the widget resolves a direction to the
    /// slice whose centre is nearest, so a label further than that from its own
    /// centre falls in a neighbour's sector and would select it. The tolerance is
    /// not a fudge factor — it is the exact width of the claim.
    pub(crate) tolerance: f32,
}

/// The node a [`RadialPlacement`] group's directions are measured from.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RadialCentre {
    /// The group's name.
    pub(crate) group: &'static str,
}

/// Which edge an [`AlignmentGroup`] holds in common.
///
/// Named **logically** rather than as left / right, per the scaffold's
/// direction-neutrality convention: a form's labels line up on their leading
/// edge, and under RTL that is the right-hand side without anything here saying
/// so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignEdge {
    /// The leading inline edge — left under LTR, right under RTL.
    InlineStart,
    /// The trailing inline edge — right under LTR, left under RTL.
    InlineEnd,
}

/// A UI element: something the gallery renders on its own and the harness checks
/// to destruction.
#[derive(Clone, Copy)]
pub(crate) struct UiElement {
    /// A stable id, used by the gallery's list and by a failing check's message.
    pub(crate) id: &'static str,
    /// One line on what this is, shown beside it in the gallery.
    pub(crate) summary: &'static str,
    /// Spawn it under `parent`, returning its own root entity.
    ///
    /// A plain `fn` pointer rather than a boxed closure so that [`ELEMENTS`] can
    /// be a `const` — the registry is a fixed list known at compile time, and
    /// keeping it one makes "is this element registered?" a question answered by
    /// reading the file.
    pub(crate) spawn: fn(&mut Commands, Entity, ElementCx) -> Entity,
}

impl core::fmt::Debug for UiElement {
    /// Hand-written because a `fn` pointer has no useful `Debug`, and the
    /// derive would print its address.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UiElement")
            .field("id", &self.id)
            .field("summary", &self.summary)
            .finish_non_exhaustive()
    }
}

/// **The registry.** Every UI element, checked by [`crate::ui_test`] and
/// rendered by [`crate::gallery`].
///
/// Add a panel or widget here and it inherits the whole suite. See the [module
/// documentation](self).
pub(crate) const ELEMENTS: &[UiElement] = &[
    UiElement {
        id: "label",
        summary: "A text label in a decorated container — the pattern a text run must be \
                  wrapped in.",
        spawn: spawn_label,
    },
    UiElement {
        id: "button",
        summary: "A focusable `bevy_ui_widgets` button that emits a `UiAction` rather than \
                  doing anything.",
        spawn: spawn_button,
    },
    UiElement {
        id: "button-row",
        summary: "Three buttons flowing in text order, wrapping when they outgrow the panel.",
        spawn: spawn_button_row,
    },
    UiElement {
        id: "field-grid",
        summary: "The build window's shape: rows of X/Y/Z fields whose columns must align \
                  however the row labels are translated.",
        spawn: spawn_field_grid,
    },
    UiElement {
        id: "text-editor",
        summary: "A multi-line `EditableText` with a caret, reachable by `Tab`.",
        spawn: spawn_text_editor,
    },
    UiElement {
        id: "text-input-line",
        summary: "The reusable single-line text field (`crate::ui_text_input`): a bordered, \
                  glyph-width-sized `EditableText` that scrolls its content horizontally.",
        spawn: crate::ui_text_input::spawn_line_specimen,
    },
    UiElement {
        id: "text-input-multiline",
        summary: "The reusable multi-line text field: newlines allowed, prose soft-wraps at a \
                  bound and scrolls vertically.",
        spawn: crate::ui_text_input::spawn_multiline_specimen,
    },
    UiElement {
        id: "text-input-float",
        summary: "The signed-decimal numeric field: accepts an optional `-`, digits and one `.`; \
                  a bad character is rejected as typed and a bad arrangement reverted.",
        spawn: crate::ui_text_input::spawn_float_specimen,
    },
    UiElement {
        id: "text-input-integer",
        summary: "The signed-integer numeric field: an optional `-` then digits.",
        spawn: crate::ui_text_input::spawn_integer_specimen,
    },
    UiElement {
        id: "text-input-unsigned",
        summary: "The non-negative-integer numeric field: digits only, the sign key rejected.",
        spawn: crate::ui_text_input::spawn_unsigned_specimen,
    },
    UiElement {
        id: "search-field",
        summary: "The reusable search-field widget (`crate::ui_search`): a single-line field in a \
                  bordered box with a leading search glyph, a placeholder shown while empty, and a \
                  trailing × clear button shown only while it holds a term. The menu-bar and \
                  inventory search boxes are two live consumers.",
        spawn: crate::ui_search::spawn_search_specimen,
    },
    UiElement {
        id: "panel",
        summary: "A composite: a titled panel with prose and a button row, bounded but not \
                  sized.",
        spawn: spawn_panel,
    },
    UiElement {
        id: "radial-menu-target",
        summary: "Right-click to open a live pie menu under the pointer. The pie is opened, used \
                  and dismissed one at a time — never a persistent card — so this is its \
                  registered form; its layout is checked directly in `crate::pie_menu`'s tests.",
        spawn: crate::pie_menu::spawn_radial_menu_target,
    },
    UiElement {
        id: "inventory-row",
        summary: "An inventory tree row: indent, expand arrow, type icon and label — an expanded \
                  folder over an indented item. The live window (`crate::inventory`) recycles \
                  this row through the virtualized list; here it is static so its layout is swept.",
        spawn: crate::inventory::spawn_inventory_row_sample,
    },
    UiElement {
        id: "floater",
        summary: "A floating window's chrome: a title bar with dock / minimize / close buttons, a \
                  content slot, and a resize grip. The live manager (`crate::floater`) makes it \
                  draggable and dockable; here it is static so its layout is swept.",
        spawn: crate::floater::spawn_floater_specimen,
    },
    UiElement {
        id: "tabs-top",
        summary: "A tabbed container with the tab strip on the top edge — three tabs fronting three \
                  panels, one shown. The reusable widget (`crate::ui_tab`); one element per \
                  placement so every orientation is swept.",
        spawn: crate::ui_tab::spawn_tabs_block_start,
    },
    UiElement {
        id: "tabs-bottom",
        summary: "The tab widget with its strip on the bottom edge — a block-axis placement, which \
                  stays bottom under RTL (only the inline axis mirrors).",
        spawn: crate::ui_tab::spawn_tabs_block_end,
    },
    UiElement {
        id: "tabs-leading",
        summary: "The tab widget with a vertical strip on the leading edge (left under LTR); it \
                  mirrors to the right under RTL with no separate code.",
        spawn: crate::ui_tab::spawn_tabs_inline_start,
    },
    UiElement {
        id: "tabs-trailing",
        summary: "The tab widget with a vertical strip on the trailing edge (right under LTR) — a \
                  placement the reference viewer cannot express, usable for LTR too, not only as \
                  an RTL mirror.",
        spawn: crate::ui_tab::spawn_tabs_inline_end,
    },
    UiElement {
        id: "menu-bar",
        summary: "A closed menu bar (`crate::menu`): a strip of pull-down buttons whose drop-downs \
                  open on click — command / check / disabled entries, separators, accelerators and \
                  a submenu. Swept closed; its opened drop-down layout is checked in the module's \
                  own tests, and it is drivable live by the gallery's right-click menu toggle.",
        spawn: crate::menu::spawn_menu_bar_specimen,
    },
    UiElement {
        id: "bottom-toolbar",
        summary: "The persistent bottom toolbar (`crate::bottom_toolbar`): a row of floater-toggle \
                  buttons in an enabled, an active (lit) and a disabled placeholder state. The live \
                  bar (bottom-anchored, wrapping upward) toggles the main floaters; here it is \
                  static so all three button states' layouts are swept.",
        spawn: crate::bottom_toolbar::spawn_bottom_toolbar_specimen,
    },
    UiElement {
        id: "minimap",
        summary: "The minimap surface (`crate::minimap`): terrain-ish backdrop, a parcel line, \
                  avatar dots and the compass labels. The live floater composites a CPU image \
                  from the world mirror; here it is static so its layout is swept.",
        spawn: crate::minimap::spawn_minimap_specimen,
    },
    UiElement {
        id: "parcel-audio-bar",
        summary: "The parcel streaming-audio cluster (`crate::parcel_audio`): the ♫ marker, a \
                  width-capped now-playing title, play and mute glyph buttons and the volume \
                  slider. The live cluster (trailing side of the bottom area) follows the \
                  agent's parcel stream; here it is static so its layout is swept.",
        spawn: crate::parcel_audio::spawn_parcel_audio_specimen,
    },
    UiElement {
        id: "emoji-picker",
        summary: "The emoji-picker floater's novel layout (`crate::emoji_picker`): a couple of grid \
                  rows of glyphs, the skin-tone swatch row and the preview line. The live floater \
                  (`Ctrl+E`) filters, groups and inserts a chosen glyph into the focused field; \
                  here it is static so its layout is swept.",
        spawn: crate::emoji_picker::spawn_emoji_picker_specimen,
    },
    UiElement {
        id: "chat-input",
        summary: "The reusable chat-input widget (`crate::chat_input`): a single-line field in a \
                  bordered box with a trailing emoji button and an inline `:`-completer. The live \
                  widget opens the picker for its field and sends on Enter; here it is static so \
                  the bar layout is swept.",
        spawn: crate::chat_input::spawn_chat_input_specimen,
    },
    UiElement {
        id: "local-chat-input",
        summary: "The reusable local-chat-input widget (`crate::local_chat_input`): the chat input \
                  plus a whisper/say/shout select box. The live widget parses `/N` channels and \
                  `/command`s and maps Shift/Ctrl+Enter to whisper/shout; here it is static so the \
                  bar layout is swept.",
        spawn: crate::local_chat_input::spawn_local_chat_input_specimen,
    },
    UiElement {
        id: "browser-view",
        summary: "The embedded-browser view (`crate::browser_widget`): a surface-backed image \
                  node with click-to-focus input routing. In the gallery the web-media engine is \
                  live and renders an offline data-URL page; in a headless test it stays the \
                  dark placeholder.",
        spawn: crate::browser_widget::spawn_browser_specimen,
    },
];

/// The accent colour on a label's leading edge.
const ACCENT_COLOR: Color = Color::srgb(0.36, 0.72, 0.98);

/// A button's resting border.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A panel's translucent backdrop.
const PANEL_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

/// A panel's widest allowed width, in logical pixels — a bound, never a size.
const PANEL_MAX_WIDTH: f32 = 420.0;

/// The sample prose an element uses where it needs a paragraph.
const SAMPLE_PROSE: &str = "A much longer label, of the length a translated string reaches when \
     the original was written in English and measured once, which is exactly the case a fixed \
     pixel rect gets wrong.";

/// Spawn a text label: the text as a plain child of its own padded, bounded box.
///
/// # An upstream measure bug shapes this, and the matrix found its real extent
///
/// `viewer-text-node-padding-measure`: `bevy_ui` resolves the wrong available
/// width for a text node. The known face of it was **never put padding or a
/// border on a `Text` node itself** — the measure over-estimates the width, fits
/// one more word per line, arrives at one fewer line, and the node is laid out
/// shorter than the text it draws, so the last line hangs out of the bottom. The
/// documented workaround was to move the decoration onto a container.
///
/// The matrix showed that workaround does not go far enough. The measure loses
/// **anything that narrows a text node's width other than its own parent's
/// padding**, and it does so silently:
///
/// | what narrows the text | measured | its box | over by |
/// | --- | --- | --- | --- |
/// | a 4 px border on the container | 388 | 384 | 4 |
/// | a 4 px sibling accent bar | 390 | 387 | 3 |
///
/// Neither is visible in English, where the wrap lands short of the boundary by
/// luck. Both show up in Arabic and under pseudolocalisation, which land on it.
///
/// So this label carries **no decoration at all** beside its text: padding on the
/// parent, and nothing else competing for the inline axis. That is a real
/// constraint on the whole UI until the upstream bug is fixed, and it is written
/// down in the bug's roadmap file rather than only here. The scaffold's `F5`
/// panel still demonstrates a logical accent bar, and still shows the artefact.
fn spawn_label(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    commands
        .spawn((
            Node {
                // A bound, not a width. Without it the label never wraps: the
                // text lays out on one unbroken line and the node grows to
                // whatever that measures. English survives by luck — the sample
                // happens to fit — while the CJK and bidi samples run 1101 and
                // 1165 px and sail straight off the edge of the window. Found by
                // `ui_test::tests::every_element_survives_every_script` the first
                // time it ran, which is the whole argument for the matrix.
                max_width: Val::Px(PANEL_MAX_WIDTH),
                ..default()
            },
            // A hanging indent: wide on the leading side, narrow elsewhere — and
            // written logically, so it mirrors rather than stranding itself on
            // the wrong side of an RTL layout.
            LogicalPadding(LogicalRect {
                inline_start: Val::Px(24.0),
                ..LogicalRect::axes(Val::Px(8.0), Val::Px(4.0))
            }),
            BackgroundColor(ACCENT_COLOR.with_alpha(0.15)),
            Name::new("label"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(cx.text(SAMPLE_PROSE)),
            cx.font(UiFont::Sans),
            TextColor(Color::WHITE),
            Name::new("label-text"),
        ))
        .id()
}

/// Spawn one focusable button carrying `label`, emitting `action` when activated.
///
/// The `.observe` is the whole of its wiring: it writes a [`UiAction`] and
/// nothing else, so the same button is real in the viewer, inert in the gallery
/// and assertable in a test.
fn button(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    tab_index: i32,
    label: &str,
    element: &'static str,
    action: &'static str,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Name::new(format!("button:{action}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(cx.text(label)),
            cx.font(UiFont::Sans),
            TextColor(Color::WHITE),
        ))
        .observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction { element, action });
            },
        )
        .id()
}

/// Spawn a single button.
fn spawn_button(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    button(commands, parent, cx, 1, "Save changes", "button", "save")
}

/// Spawn three buttons flowing in text order.
///
/// Three, not two: with two focusable nodes a tab cycle is its own reverse, so
/// `Tab` and `Shift+Tab` are indistinguishable and neither order nor direction
/// is observable.
fn spawn_button_row(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let row = commands
        .spawn((
            Node {
                // Wraps rather than overflowing once the labels outgrow the
                // panel — the row-level half of the content-driven convention.
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(8.0),
                ..row(Val::Px(8.0))
            },
            Name::new("button-row"),
            ChildOf(parent),
        ))
        .id();
    for (index, (label, action)) in [
        ("Save changes", "save"),
        ("Discard", "discard"),
        ("Cancel", "cancel"),
    ]
    .into_iter()
    .enumerate()
    {
        let tab_index = i32::try_from(index).unwrap_or(0).saturating_add(1);
        button(commands, row, cx, tab_index, label, "button-row", action);
    }
    row
}

/// The field columns of [`spawn_field_grid`], each an [`AlignmentGroup`].
///
/// Named per column rather than one group for all of them, because the claim is
/// that each column is internally straight — `x` under `x` — not that all nine
/// fields share one edge, which would be false and would fail immediately.
const FIELD_COLUMNS: [&str; 3] = ["field-x", "field-y", "field-z"];

/// Spawn the build window's shape: rows of X/Y/Z fields, one row per property.
///
/// **The element that exists for the alignment check.** The reference viewer's
/// build window puts Position, Rotation and Scale each on a row of three numeric
/// fields, and the fields have to form three straight columns. In English they
/// do so by luck — `Position`, `Rotation` and `Scale` are all about as wide as
/// each other — and per-row flexbox would pass every eyeball test. It breaks in
/// the first language where one of those words runs long: that row's label is
/// wider, its fields start further along, and the columns go ragged. The same
/// happens under RTL, at a larger UI font, and under pseudolocalisation.
///
/// So this is a **CSS grid**, not a stack of rows. One `auto` label column sized
/// to the *widest* label across every row, then three equal field columns — and
/// every row's fields inherit the same track, so they align by construction in
/// any language rather than by coincidence in one. The declared
/// [`AlignmentGroup`]s are what hold that claim to account across the whole
/// matrix, and what will catch the next panel that reaches for per-row flex
/// instead.
fn spawn_field_grid(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let grid = commands
        .spawn((
            Node {
                display: Display::Grid,
                // The label track is `auto` — as wide as the widest label in any
                // language — and the three field tracks are equal to each other.
                // This one line is what makes the columns straight; a per-row
                // `row()` would let every row size its own label.
                grid_template_columns: vec![
                    GridTrack::auto(),
                    GridTrack::flex(1.0),
                    GridTrack::flex(1.0),
                    GridTrack::flex(1.0),
                ],
                column_gap: Val::Px(8.0),
                row_gap: Val::Px(6.0),
                ..default()
            },
            Name::new("field-grid"),
            ChildOf(parent),
        ))
        .id();
    for (label, values) in [
        ("Position", ["128.0", "128.0", "22.5"]),
        ("Rotation", ["0.0", "0.0", "45.0"]),
        ("Scale", ["0.5", "0.5", "0.5"]),
    ] {
        commands.spawn((
            Text::new(cx.text(label)),
            cx.font(UiFont::Sans),
            TextColor(Color::srgb(0.80, 0.85, 0.92)),
            // The labels are a column too, and they must end flush against the
            // fields — the trailing edge, which under RTL is the left one. The
            // grid's single `auto` track is what makes that true in any language;
            // the declaration is what notices if someone replaces the grid with
            // per-row flex and quietly breaks it in German.
            AlignmentGroup {
                group: "field-label",
                edge: AlignEdge::InlineEnd,
            },
            Name::new(format!("field-label:{label}")),
            ChildOf(grid),
        ));
        for (column, value) in FIELD_COLUMNS.iter().zip(values) {
            commands
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(BUTTON_BORDER),
                    BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
                    // The declaration: this column's fields share a leading edge,
                    // in every script, at every scale, however the row labels
                    // translate.
                    AlignmentGroup {
                        group: column,
                        edge: AlignEdge::InlineStart,
                    },
                    Name::new(format!("{column}:{label}")),
                    ChildOf(grid),
                ))
                .with_child((
                    // A numeric field: never translated, so it stays literal
                    // while everything around it moves.
                    Text::new(value),
                    cx.font(UiFont::Mono),
                    TextColor(Color::WHITE),
                ));
        }
    }
    grid
}

/// Spawn a multi-line text editor with a caret, reachable by `Tab`.
fn spawn_text_editor(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let mut editor = EditableText::new(cx.text(SAMPLE_PROSE));
    editor.allow_newlines = true;
    editor.visible_lines = Some(3.0);
    commands
        .spawn((
            editor,
            cx.font(UiFont::Sans),
            TextColor(Color::WHITE),
            TextCursorStyle::default(),
            // An editor showing three lines of a longer text clips the rest, and
            // scrolls the caret into view rather than growing without bound —
            // so its text is legitimately sliced at the boundary and it claims
            // the exception rather than being quietly special-cased inside the
            // check.
            TextMayClip {
                reason: "an editor scrolls its content to follow the caret, so the text at the \
                         visible-line boundary is cut by design",
            },
            TabIndex(0),
            Node {
                // A bound, not a width: a fixed width here used to overflow the
                // containing panel's `max_width` by exactly its padding.
                max_width: Val::Px(PANEL_MAX_WIDTH),
                border: UiRect::all(Val::Px(2.0)),
                padding: UiRect::all(Val::Px(6.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
            Name::new("text-editor"),
            ChildOf(parent),
        ))
        .id()
}

/// Spawn a titled panel: the composite the viewer's real panels take after.
fn spawn_panel(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let panel = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(12.0)),
                // A bound, not a size: the panel is as wide as its content needs
                // and wraps beyond this.
                max_width: Val::Px(PANEL_MAX_WIDTH),
                ..column(Val::Px(8.0))
            },
            BackgroundColor(PANEL_BACKGROUND),
            Name::new("panel"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(cx.text("Object properties")),
        cx.font(UiFont::Sans),
        TextColor(Color::srgb(0.80, 0.85, 0.92)),
        Name::new("panel-title"),
        ChildOf(panel),
    ));
    spawn_label(commands, panel, cx);
    spawn_button_row(commands, panel, cx);
    panel
}

#[cfg(test)]
mod tests {
    use super::{ELEMENTS, SCRIPTS, SampleText, ScriptSample};
    use pretty_assertions::assert_eq;

    /// Ids are what a failing check names and what the gallery lists, so a
    /// duplicate would make one element's failure indistinguishable from
    /// another's.
    #[test]
    fn element_ids_are_unique() {
        let mut ids: Vec<&str> = ELEMENTS.iter().map(|element| element.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "two registered elements share an id");
    }

    /// The registry has to be non-empty, or every matrix test below passes by
    /// iterating over nothing — the failure mode where a green suite means the
    /// suite ran out of work rather than found none.
    #[test]
    fn the_registry_is_not_empty() {
        assert!(!ELEMENTS.is_empty(), "no UI elements are registered");
        assert!(!SCRIPTS.is_empty(), "no scripts are registered");
    }

    /// A script swap keeps a label label-sized and prose prose-sized, or it would
    /// test a different layout problem than the string it replaced posed.
    #[test]
    fn a_script_sample_matches_the_length_class_of_what_it_replaces() {
        const SAMPLE: ScriptSample = ScriptSample {
            name: "Test",
            short: "short",
            long: "a considerably longer sample string standing in for prose",
        };
        let cell = SampleText::Script(&SAMPLE);
        assert_eq!(cell.apply("Save"), "short");
        assert_eq!(
            cell.apply(
                "A much longer label, of the length a translated string reaches when the \
                 original was written in English"
            ),
            "a considerably longer sample string standing in for prose"
        );
    }

    /// Native is the identity — the baseline every other cell is compared
    /// against, so it must not quietly transform anything.
    #[test]
    fn the_native_cell_changes_nothing() {
        assert_eq!(SampleText::Native.apply("Save changes"), "Save changes");
    }

    /// Every script sample must actually be in the script it claims, at both
    /// lengths, and the long one must be long enough to force a wrap. A sample
    /// silently left as English would make its whole column of the matrix a
    /// re-run of Latin.
    #[test]
    fn every_script_sample_is_non_empty_and_distinct() {
        for sample in SCRIPTS {
            assert!(
                !sample.short.is_empty(),
                "{}: empty short sample",
                sample.name
            );
            assert!(
                !sample.long.is_empty(),
                "{}: empty long sample",
                sample.name
            );
            assert!(
                sample.long.chars().count() > sample.short.chars().count(),
                "{}: the long sample must be longer than the short one",
                sample.name
            );
        }
        let mut shorts: Vec<&str> = SCRIPTS.iter().map(|sample| sample.short).collect();
        let total = shorts.len();
        shorts.sort_unstable();
        shorts.dedup();
        assert_eq!(
            shorts.len(),
            total,
            "two scripts share a sample — one of them is not in the script it claims"
        );
    }
}
