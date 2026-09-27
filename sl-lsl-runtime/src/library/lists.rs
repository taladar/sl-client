//! Library tranche: strings and lists (`server-lsl-lib-strings-lists`).

use crate::library::CallError;
use crate::value::Element;

/// `llGetListLength`: the number of elements.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_list_length<C>(_ctx: &mut C, list: Vec<Element>) -> Result<i32, CallError> {
    Ok(i32::try_from(list.len()).unwrap_or(i32::MAX))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn counts_elements() {
        assert_eq!(ll_get_list_length(&mut (), Vec::new()), Ok(0));
        assert_eq!(
            ll_get_list_length(&mut (), vec![Element::Integer(1), Element::Float(2.0)]),
            Ok(2)
        );
    }
}
