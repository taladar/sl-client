//! Which library functions exist: the one list of implementations and
//! deliberate stubs. Add a function here when it is written; the
//! registration will not compile unless its signature is the table's.

use crate::library::{control, lists, math};

crate::registry! {
    implemented {
        LlAbs => math::ll_abs,
        LlFabs => math::ll_fabs,
        LlGetListLength => lists::ll_get_list_length,
        LlResetScript => control::ll_reset_script,
        LlSleep => control::ll_sleep,
    }
    stubbed {}
}
