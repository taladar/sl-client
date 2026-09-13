//! Seeding an edit form's widgets from a draft, without eating what the
//! resident is typing.
//!
//! Both big floaters here — [`about_land`](crate::about_land) and
//! [`about_region`](crate::about_region) — hold a **draft** record, let the
//! resident change it, and send the whole thing on **Apply**. A record the grid
//! pushes in the meantime is folded into the draft field by field
//! (`merge_unedited`), so an unapplied edit survives it.
//!
//! Text fields are the hole in that. They are not mirrored into the draft until
//! Apply reads them, so the merge cannot see typing in flight and a reseed
//! would simply overwrite it. [`FieldSeed`] and [`seed_one_field`] are how the
//! widgets get the same protection: a field still reading exactly what it was
//! last **given** is one nobody has typed in and may be rewritten; anything
//! else is the resident's and is left alone.
//!
//! The comparison is against what the widget was last given, not against the
//! merge's base — by the time a push is being folded in, the base has already
//! advanced to it.

use bevy::prelude::*;
use bevy::text::EditableText;

use crate::ui_combo::ComboSelection;

/// How much of an edit form's text the next seeding pass may rewrite.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldSeed {
    /// Leave the fields alone.
    #[default]
    None,
    /// Rewrite every field — a fresh subject, whose text has nothing to do with
    /// what the widgets are showing.
    All,
    /// Rewrite only a field whose text still equals what was last seeded into
    /// it, leaving anything the resident has typed since.
    Unedited,
}

impl FieldSeed {
    /// What the previous pass wrote, as this mode wants it compared: `None` in
    /// [`All`](Self::All), where the subject itself changed and the old text
    /// means nothing.
    pub(crate) const fn previous<T>(self, shown: Option<&T>) -> Option<&T> {
        match self {
            Self::None | Self::All => None,
            Self::Unedited => shown,
        }
    }
}

/// A field's current text, if it exists.
fn field_text(fields: &Query<&mut EditableText>, field: Option<Entity>) -> Option<String> {
    fields
        .get(field?)
        .ok()
        .map(|editable| editable.value().to_string())
}

/// Whether `field`'s current text is exactly `value`.
///
/// A missing field reads as matching, so a form built without it is seeded
/// rather than skipped.
fn field_reads(fields: &Query<&mut EditableText>, field: Option<Entity>, value: &str) -> bool {
    field_text(fields, field).is_none_or(|text| text == value)
}

/// Write `want` into one text field unless the resident has typed in it, and
/// record in `left` what the field was last **given**.
///
/// `previous` is what the last pass gave it, or `None` to rewrite
/// unconditionally.
///
/// # A declined write gives the field nothing
///
/// So `left` keeps `previous` rather than taking the resident's text. Recording
/// what the widget currently *reads* would make the very next push overwrite it:
/// the comparison would then be the typing against itself, which matches, which
/// says "nobody has typed here". One push would be survived and the second would
/// not — and a form left open in a busy estate sees a great many pushes.
pub(crate) fn seed_one_field(
    fields: &mut Query<&mut EditableText>,
    field: Option<Entity>,
    want: &str,
    previous: Option<&str>,
    left: &mut String,
) {
    if let Some(previous) = previous
        && !field_reads(fields, field, previous)
    {
        left.clear();
        left.push_str(previous);
        return;
    }
    set_field_text(fields, field, want);
}

/// Seed a text field's content in place, skipping an actively-edited field.
#[expect(
    clippy::cmp_owned,
    reason = "the editor's SplitString has no borrow-free comparison against &str; this guard runs \
              only on a discrete reseed, not per frame"
)]
fn set_field_text(fields: &mut Query<&mut EditableText>, field: Option<Entity>, value: &str) {
    if let Some(field) = field
        && let Ok(mut editable) = fields.get_mut(field)
        && !editable.is_composing()
        && editable.value().to_string() != value
    {
        editable.editor_mut().set_text(value);
    }
}

/// Set a combo's selection in place (a programmatic write emits no `ComboChanged`).
pub(crate) fn set_combo(
    combos: &mut Query<&mut ComboSelection>,
    combo: Option<Entity>,
    active: usize,
) {
    if let Some(combo) = combo
        && let Ok(mut selection) = combos.get_mut(combo)
        && selection.active != active
    {
        selection.active = active;
    }
}
