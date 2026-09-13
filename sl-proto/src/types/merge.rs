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
///     #[expect(clippy::float_cmp, reason = "…")]
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
macro_rules! merge_unedited {
    (
        $(#[$attr:meta])*
        $ty:ident { $($field:ident),+ $(,)? }
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
                    if self.$field == base.$field && self.$field != *$field {
                        self.$field = $field.clone();
                        moved = true;
                    }
                )+
                moved
            }
        }
    };
}

pub(crate) use merge_unedited;
