// probe: comms/link-messages
// script: Receiver
// order: 2
// drive: none
//
// The Sender's neighbour in prim A (see there): it says it is ready by
// link message to LINK_THIS and prints every message it gets.
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

default
{
    state_entry()
    {
        llMessageLinked(LINK_THIS, 1, "ready", "");
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num >= 50)
        {
            emit("got", " script=Receiver target=" + (string)(num - 100)
                + " sender=" + (string)sender
                + " text_length=" + (string)llStringLength(text)
                + " id=" + q(llGetSubString((string)id, 0, 39)));
        }
    }
}
