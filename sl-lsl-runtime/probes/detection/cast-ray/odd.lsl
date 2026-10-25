// probe: detection/cast-ray
// script: Odd
// order: 3
// drive: none
//
// A ray of the Caster's (see there) in a script of its own, since it may
// stop the script that casts it: llCastRay with RC_MAX_HITS and no value
// after it. The Caster asks for it by link message once its own rays are
// done; this prints the answer, if one comes.
string ID = "detection/cast-ray";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer number, string text, key id)
    {
        if (text != "odd")
        {
            return;
        }
        vector here = llGetPos();
        list result = llCastRay(here + <0.5, 0.0, 0.0>, here + <15.0, 0.0, 0.0>, [RC_MAX_HITS]);
        emit("ray", " step=missing-value options=\"RC_MAX_HITS\" length="
            + (string)llGetListLength(result)
            + " status=" + (string)llList2Integer(result, -1));
    }
}
