// probe: prim/link-refusals
// script: Unpermitted
// order: 3
// drive: none
//
// The Leader's second script in prim A (see there), which never asks for a
// permission. Told "unpermitted" by link message, it calls llBreakLink(2),
// llBreakLink(1) and llBreakAllLinks, each after a `try` record, so the
// runner notes beside each what the grid said.
string ID = "prim/link-refusals";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string str, key id)
    {
        if (str != "unpermitted")
        {
            return;
        }
        emit("try", " label=break-2-unpermitted");
        llBreakLink(2);
        emit("try", " label=break-1-unpermitted");
        llBreakLink(1);
        emit("try", " label=break-all-unpermitted");
        llBreakAllLinks();
        emit("unpermitted-done", " prims=" + (string)llGetNumberOfPrims());
    }
}
