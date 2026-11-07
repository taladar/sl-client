//! Reading a rule list one value at a time, as each grid reads it: which
//! value types a rule takes for which, and the words it refuses the rest in
//! (book chapter `gridspec/lsl.md`, § Rule lists).
//!
//! Second Life is strict: an integer must be an integer, a float may be an
//! integer, a string must be a string, and a value of another type stops the
//! rule list with `arg #2 (glow) float expected but string given.` — save a
//! key given for a string, which stops it without a word (`prim/params-rules`,
//! `name-as-key`). OpenSim converts what `LSL_Types.list`'s getters convert —
//! a string or a key to a number, anything to a string — and refuses a vector
//! where it wants a float with `arg #1 - parameter 3 must be float`.

use sl_types::lsl::{Rotation, Vector};

use crate::value::Element;
use crate::vm::ScriptCtx;

/// Why a rule list stopped before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// A rule had too few values: it does nothing and neither do the rules
    /// after it, silently, on both grids.
    Short,
    /// A value was refused, and said so; the rules after it do not run.
    Refused,
    /// A value was refused without a word (Second Life's key for a string).
    Silent,
}

/// What a rule wants of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    /// An integer.
    Integer,
    /// A float.
    Float,
    /// A string.
    String,
    /// A vector.
    Vector,
    /// A rotation.
    Rotation,
}

impl Want {
    /// Its name in a refusal, the same on both grids.
    const fn name(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::String => "string",
            Self::Vector => "vector",
            Self::Rotation => "rotation",
        }
    }
}

/// The name Second Life gives an element's type in a refusal.
const fn given(element: &Element) -> &'static str {
    match element {
        Element::Integer(_) => "integer",
        Element::Float(_) => "float",
        Element::String(_) => "string",
        Element::Key(_) => "key",
        Element::Vector(_) => "vector",
        Element::Rotation(_) => "rotation",
    }
}

/// A rule list being read: where the reading is, and which rule it is in,
/// for the words of a refusal.
#[derive(Debug)]
pub(crate) struct Reader<'list> {
    /// The rule list.
    list: &'list [Element],
    /// The index of the next value.
    pub(crate) at: usize,
    /// The function the rules were given to, which a refusal names.
    pub(crate) function: &'static str,
    /// The rule being read, counted from 1 across the whole list.
    pub(crate) number: u32,
    /// The index of the rule's first value after its code.
    start: usize,
    /// The rule's name, as Second Life's refusal writes it in brackets
    /// (`PRIM_GLOW`); empty for a refusal that names none.
    pub(crate) name: &'static str,
    /// Whether the region imitates OpenSim.
    opensim: bool,
}

impl<'list> Reader<'list> {
    /// A reader at the start of `list`, for `function` under `ctx`'s flavour.
    pub(crate) const fn new(
        ctx: &ScriptCtx<'_>,
        function: &'static str,
        list: &'list [Element],
    ) -> Self {
        Self {
            list,
            at: 0,
            function,
            number: 0,
            start: 0,
            name: "",
            opensim: ctx.flavour().is_opensim(),
        }
    }

    /// Whether every value has been read.
    pub(crate) const fn done(&self) -> bool {
        self.at >= self.list.len()
    }

    /// How many values are left.
    pub(crate) const fn remaining(&self) -> usize {
        self.list.len().saturating_sub(self.at)
    }

    /// The next value, unread.
    pub(crate) fn peek(&self) -> Option<&'list Element> {
        self.list.get(self.at)
    }

    /// The next value, read.
    fn next(&mut self) -> Option<&'list Element> {
        let element = self.list.get(self.at)?;
        self.at = self.at.saturating_add(1);
        Some(element)
    }

    /// Starts the next rule: counts it, and names it `name` for refusals.
    pub(crate) const fn begin(&mut self, name: &'static str) {
        self.number = self.number.saturating_add(1);
        self.start = self.at;
        self.name = name;
    }

    /// Renames the rule being read (a `PRIM_TYPE`'s shape names itself).
    pub(crate) const fn rename(&mut self, name: &'static str) {
        self.name = name;
    }

    /// Whether at least `count` values are left; when not, the rule is
    /// [`Stop::Short`].
    ///
    /// # Errors
    ///
    /// [`Stop::Short`].
    pub(crate) const fn need(&self, count: usize) -> Result<(), Stop> {
        if self.remaining() < count {
            Err(Stop::Short)
        } else {
            Ok(())
        }
    }

    /// The index of the value just read within its rule, counted from 0
    /// after the code.
    const fn argument(&self) -> usize {
        self.at.saturating_sub(self.start).saturating_sub(1)
    }

    /// Refuses the value just read, which should have been `want`; `label`
    /// is Second Life's name for the argument, if it gives one.
    fn refuse(
        &self,
        ctx: &mut ScriptCtx<'_>,
        want: Want,
        label: Option<&str>,
        element: &Element,
    ) -> Stop {
        let argument = self.argument();
        let text = if self.opensim {
            let rule = if self.name.is_empty() {
                String::new()
            } else {
                format!(" -> {}", self.name)
            };
            format!(
                "{}: Error running rule #{}{rule}: arg #{argument} - parameter {} must be {}",
                self.function,
                self.number,
                argument.saturating_add(2),
                want.name(),
            )
        } else {
            let label = label.map_or_else(String::new, |label| format!(" ({label})"));
            format!(
                "{} error running rule #{} ({}): arg #{}{label} {} expected but {} given.",
                self.function,
                self.number,
                self.name,
                argument.saturating_add(1),
                want.name(),
                given(element),
            )
        };
        ctx.refuse_to_owner(text);
        Stop::Refused
    }

    /// Says `detail` against the rule being read in the flavour's words:
    /// Second Life's `… error running rule #1 (PRIM_FULLBRIGHT): <detail>`,
    /// OpenSim's `…: Error running rule #1 -> PRIM_ALPHA_MODE: arg #k -
    /// <detail>` (which also costs the script a second).
    pub(crate) fn error(&self, ctx: &mut ScriptCtx<'_>, detail: &str) {
        let text = if self.opensim {
            format!(
                "{}: Error running rule #{} -> {}: arg #{} - {detail}",
                self.function,
                self.number,
                self.name,
                self.argument(),
            )
        } else {
            format!(
                "{} error running rule #{} ({}): {detail}",
                self.function, self.number, self.name,
            )
        };
        ctx.refuse_to_owner(text);
    }

    /// Second Life's `… warning running rule #1 (PRIM_TYPE_BOX): <detail>`:
    /// the rule carries on.
    pub(crate) fn warn(&self, ctx: &mut ScriptCtx<'_>, detail: &str) {
        ctx.refuse_to_owner(format!(
            "{} warning running rule #{} ({}): {detail}",
            self.function, self.number, self.name,
        ));
    }

    /// The rule list's code at the next value: an integer, or on OpenSim a
    /// string or key read as one. [`None`] for a code Second Life would not
    /// read (any other type) and for the end of the list.
    pub(crate) fn code(&mut self) -> Option<i32> {
        let element = self.next()?;
        match element {
            Element::Integer(code) => Some(*code),
            Element::String(text) | Element::Key(text) if self.opensim => {
                Some(crate::parse::integer(text))
            }
            _ => None,
        }
    }

    /// The next value as an integer.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list; [`Stop::Refused`] for a value
    /// the grid will not read as one.
    pub(crate) fn integer(
        &mut self,
        ctx: &mut ScriptCtx<'_>,
        label: Option<&str>,
    ) -> Result<i32, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        match element {
            Element::Integer(value) => Ok(*value),
            Element::String(text) | Element::Key(text) if self.opensim => {
                Ok(crate::parse::integer(text))
            }
            other => Err(self.refuse(ctx, Want::Integer, label, other)),
        }
    }

    /// The next value as a float: an integer is taken on both grids.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list; [`Stop::Refused`] for a value
    /// the grid will not read as one.
    pub(crate) fn float(
        &mut self,
        ctx: &mut ScriptCtx<'_>,
        label: Option<&str>,
    ) -> Result<f32, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        match element {
            Element::Float(value) => Ok(*value),
            Element::Integer(value) => Ok(crate::num::int_to_float(*value)),
            Element::String(text) | Element::Key(text) if self.opensim => {
                Ok(crate::parse::float(text))
            }
            other => Err(self.refuse(ctx, Want::Float, label, other)),
        }
    }

    /// The next value as a string: on OpenSim whatever it is, as text.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list; on Second Life
    /// [`Stop::Silent`] for a key and [`Stop::Refused`] for anything else
    /// that is no string.
    pub(crate) fn string(
        &mut self,
        ctx: &mut ScriptCtx<'_>,
        label: Option<&str>,
    ) -> Result<String, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        match element {
            Element::String(text) => Ok(text.clone()),
            other if self.opensim => Ok(crate::cast::element_to_string(other)),
            Element::Key(_) => Err(Stop::Silent),
            other => Err(self.refuse(ctx, Want::String, label, other)),
        }
    }

    /// The next value as a texture, map or sound: a string or a key on both
    /// grids, and on OpenSim anything as text.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list; on Second Life
    /// [`Stop::Refused`] for a value that is neither a string nor a key.
    pub(crate) fn name_or_key(
        &mut self,
        ctx: &mut ScriptCtx<'_>,
        label: Option<&str>,
    ) -> Result<String, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        match element {
            Element::String(text) | Element::Key(text) => Ok(text.clone()),
            other if self.opensim => Ok(crate::cast::element_to_string(other)),
            other => Err(self.refuse(ctx, Want::String, label, other)),
        }
    }

    /// The next value as a vector.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list; [`Stop::Refused`] for any
    /// other type.
    pub(crate) fn vector(
        &mut self,
        ctx: &mut ScriptCtx<'_>,
        label: Option<&str>,
    ) -> Result<Vector, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        match element {
            Element::Vector(value) => Ok(value.clone()),
            other => Err(self.refuse(ctx, Want::Vector, label, other)),
        }
    }

    /// The next value as a rotation.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list; [`Stop::Refused`] for any
    /// other type.
    pub(crate) fn rotation(
        &mut self,
        ctx: &mut ScriptCtx<'_>,
        label: Option<&str>,
    ) -> Result<Rotation, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        match element {
            Element::Rotation(value) => Ok(value.clone()),
            other => Err(self.refuse(ctx, Want::Rotation, label, other)),
        }
    }

    /// The next value as a flag, as OpenSim's phantom, physics and
    /// temporary rules read it: true for exactly the text `1`.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list.
    pub(crate) fn flag_text(&mut self) -> Result<bool, Stop> {
        let element = self.next().ok_or(Stop::Short)?;
        Ok(crate::cast::element_to_string(element) == "1")
    }

    /// Skips the next value, whatever it is.
    ///
    /// # Errors
    ///
    /// [`Stop::Short`] at the end of the list.
    pub(crate) fn skip(&mut self) -> Result<(), Stop> {
        self.next().map(|_element| ()).ok_or(Stop::Short)
    }
}
