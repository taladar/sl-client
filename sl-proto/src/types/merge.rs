//! The three-way merge an edit form needs to converge with the grid.
//!
//! Every "settings" message in this protocol family carries the **whole**
//! record: a `ParcelPropertiesUpdate` re-states all eighteen parcel fields, a
//! `setregioninfo` all nine region ones. A form populated once when its floater
//! opened therefore re-asserts every field as it stood then, silently undoing
//! whatever another resident changed in between. Nothing on the grid side can
//! stop that — a simulator cannot tell a re-asserted field from an unchanged
//! one, and Second Life has no edit lock to have prevented the overlap — so
//! convergence is the viewer's job, and [`merge_unedited`] is the shape of it.
//!
//! The same walk over every field, for four record types, is what this macro
//! exists to avoid writing four times.

/// Give a record type a `merge_unedited` three-way merge.
///
/// Invoked as the type's name followed by **every one of its fields**:
///
/// ```ignore
/// merge_unedited! {
///     /// Per-type prose, appended to the shared documentation below.
///     RegionDebugUpdate { disable_scripts, disable_collisions, disable_physics }
/// }
/// ```
///
/// The field list is checked against the type twice over: it becomes a
/// destructuring pattern **without** a `..` rest, so a field the type gained
/// and this list did not is a compile error, and every listed name is used, so
/// one that is not a field is an error too. A forgotten field is the hazard
/// this whole conversion family has, and neither list is allowed to drift in
/// silence.
///
/// Fields are compared exactly, floats included, through the generic
/// `merge_whole`: the question is whether a value moved at all, and both sides
/// come from the same decode of the same wire field, so a difference of any
/// size is an edit (which is also why an invocation needs no `float_cmp`
/// expectation).
///
/// A field written `name by merge_fn` is merged by `merge_fn(&mut form, &base,
/// &fresh) -> bool` instead of whole: for a bit set such as a parcel's flags,
/// where each bit is its own checkbox and a resident's tick on one must not
/// pin all the others (`merge_unedited_bits`).
macro_rules! merge_unedited {
    (
        $(#[$attr:meta])*
        $ty:ident { $($field:ident $(by $merge:path)?),+ $(,)? }
    ) => {
        impl $ty {
            /// Carry a freshly read record into every field of this form the
            /// resident has **not** edited, and report whether anything moved.
            ///
            /// `base` is the record this form was seeded from, `self` is the
            /// form as it stands, and `fresh` is the record the grid most
            /// recently reported. A field of `self` that still equals `base` is
            /// one nobody here has touched, so it is the grid's to state and
            /// takes the pushed value; a field that has moved away from `base`
            /// is the resident's pending edit and is kept.
            ///
            /// The caller advances `base` to `fresh` afterwards, so a field the
            /// resident edited to the value the grid already holds stops
            /// counting as an edit — the two agree, and there is nothing left
            /// to protect.
            $(#[$attr])*
            pub fn merge_unedited(&mut self, base: &Self, fresh: &Self) -> bool {
                let Self { $($field),+ } = fresh;
                let mut moved = false;
                $(
                    if $crate::types::merge::merge_field!(
                        &mut self.$field, &base.$field, $field $(, $merge)?
                    ) {
                        moved = true;
                    }
                )+
                moved
            }
        }
    };
}

/// One field of a [`merge_unedited!`] walk: whole by default, or by the named
/// merge function. Evaluates to whether the field moved.
macro_rules! merge_field {
    ($form:expr, $base:expr, $fresh:expr) => {
        $crate::types::merge::merge_whole($form, $base, $fresh)
    };
    ($form:expr, $base:expr, $fresh:expr, $merge:path) => {
        $merge($form, $base, $fresh)
    };
}

pub(crate) use {merge_field, merge_unedited};

/// Merge one field whole: an unedited field (still equal to `base`) takes
/// `fresh`. Returns whether it moved.
pub(crate) fn merge_whole<T: PartialEq + Clone>(form: &mut T, base: &T, fresh: &T) -> bool {
    if *form == *base && *form != *fresh {
        form.clone_from(fresh);
        true
    } else {
        false
    }
}

/// Merge parcel flags **per bit**: every bit the resident has not toggled away
/// from `base` takes its value from `fresh`; the toggled ones are kept.
/// Returns whether any bit moved.
///
/// A parcel's flags are a dozen independent checkboxes sharing one wire field.
/// Merged whole, one pending tick (say *Allow damage*) would hold every other
/// bit at its seeded value — including the access-list bit OpenSim flips by
/// itself when a list is saved, which the next **Apply** would then put back.
pub(crate) const fn merge_unedited_bits(
    form: &mut sl_wire::ParcelFlags,
    base: &sl_wire::ParcelFlags,
    fresh: &sl_wire::ParcelFlags,
) -> bool {
    let edited = form.bits() ^ base.bits();
    let merged = (form.bits() & edited) | (fresh.bits() & !edited);
    if merged == form.bits() {
        return false;
    }
    *form = sl_wire::ParcelFlags::from_bits(merged);
    true
}

#[cfg(test)]
mod tests {
    use super::merge_unedited_bits;
    use pretty_assertions::assert_eq;
    use sl_wire::ParcelFlags;

    /// A bit the resident ticked is kept while the grid moves another one.
    #[test]
    fn an_edited_bit_does_not_pin_the_others() {
        let base = ParcelFlags::ALLOW_FLY;
        let mut form = base.union(ParcelFlags::ALLOW_DAMAGE);
        let fresh = base.union(ParcelFlags::USE_ACCESS_LIST);
        assert!(merge_unedited_bits(&mut form, &base, &fresh));
        assert_eq!(
            form,
            ParcelFlags::ALLOW_FLY
                .union(ParcelFlags::ALLOW_DAMAGE)
                .union(ParcelFlags::USE_ACCESS_LIST)
        );
    }

    /// The grid clearing a bit the resident set keeps the resident's tick,
    /// and clearing one nobody touched clears it.
    #[test]
    fn a_cleared_bit_follows_only_where_unedited() {
        let base = ParcelFlags::USE_BAN_LIST;
        let mut form = base.union(ParcelFlags::ALLOW_VOICE);
        let fresh = ParcelFlags::from_bits(0);
        assert!(merge_unedited_bits(&mut form, &base, &fresh));
        assert_eq!(form, ParcelFlags::ALLOW_VOICE);
    }

    /// Nothing new from the grid moves nothing.
    #[test]
    fn an_unchanged_record_reports_no_move() {
        let base = ParcelFlags::ALLOW_FLY;
        let mut form = base.union(ParcelFlags::ALLOW_DAMAGE);
        assert!(!merge_unedited_bits(&mut form, &base, &base));
        assert_eq!(form, base.union(ParcelFlags::ALLOW_DAMAGE));
    }
}
