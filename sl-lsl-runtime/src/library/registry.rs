//! Which library functions exist: the one list of implementations and
//! deliberate stubs. Add a function here when it is written; the
//! registration will not compile unless its signature is the table's.

use crate::library::{control, detection, lists, math};

crate::registry! {
    implemented {
        LlAbs => math::ll_abs,
        LlFabs => math::ll_fabs,
        LlGetListLength => lists::ll_get_list_length,
        LlResetScript => control::ll_reset_script,
        LlGetStartParameter => control::ll_get_start_parameter,
        LlSetTimerEvent => control::ll_set_timer_event,
        LlDetectedKey => detection::ll_detected_key,
        LlDetectedName => detection::ll_detected_name,
        LlDetectedOwner => detection::ll_detected_owner,
        LlDetectedType => detection::ll_detected_type,
        LlDetectedGroup => detection::ll_detected_group,
        LlDetectedLinkNumber => detection::ll_detected_link_number,
        LlDetectedPos => detection::ll_detected_pos,
        LlDetectedRot => detection::ll_detected_rot,
        LlDetectedVel => detection::ll_detected_vel,
        LlDetectedGrab => detection::ll_detected_grab,
        LlDetectedTouchFace => detection::ll_detected_touch_face,
        LlDetectedTouchST => detection::ll_detected_touch_st,
        LlDetectedTouchUV => detection::ll_detected_touch_uv,
        LlDetectedTouchPos => detection::ll_detected_touch_pos,
        LlDetectedTouchNormal => detection::ll_detected_touch_normal,
        LlDetectedTouchBinormal => detection::ll_detected_touch_binormal,
        LlSleep => control::ll_sleep,
        LlGetScriptState => control::ll_get_script_state,
    }
    stubbed {}
}
