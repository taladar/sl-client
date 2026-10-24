// probe: comms/link-messages
// script: Sender
// order: 1
// drive: none
// unordered: got
//
// Who a link message reaches in a prim linked to nothing. Two scripts
// share prim A: this one, which sends, and the Receiver, saved after it,
// which says by link message when it is ready. This script then sends one
// message to each target — LINK_THIS, LINK_SET, LINK_ALL_OTHERS,
// LINK_ALL_CHILDREN, LINK_ROOT, and the link numbers 0, 1 and 2 — with the
// target in the number (plus 100), then one to LINK_THIS whose key is a
// string that is no key and whose string is 3000 characters long. Both
// scripts print every message they get, with the sender's link number.
string ID = "comms/link-messages";

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

send(integer target)
{
    emit("sent", " target=" + (string)target);
    llMessageLinked(target, 100 + target, "", "");
}

default
{
    state_entry()
    {
        begin();
        emit("prim", " link_number=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims());
        llSetTimerEvent(10.0);
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num == 1 && text == "ready")
        {
            send(LINK_THIS);
            send(LINK_SET);
            send(LINK_ALL_OTHERS);
            send(LINK_ALL_CHILDREN);
            send(LINK_ROOT);
            send(0);
            send(1);
            send(2);
            string long;
            integer i;
            for (i = 0; i < 300; ++i)
            {
                long += "0123456789";
            }
            emit("sent", " target=" + (string)LINK_THIS + " payload=odd");
            llMessageLinked(LINK_THIS, 100 + LINK_THIS, long, "not a key");
            llSetTimerEvent(3.0);
            return;
        }
        if (num >= 50)
        {
            emit("got", " script=Sender target=" + (string)(num - 100)
                + " sender=" + (string)sender
                + " text_length=" + (string)llStringLength(text)
                + " id=" + q(llGetSubString((string)id, 0, 39)));
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        emit("end", "");
    }
}
