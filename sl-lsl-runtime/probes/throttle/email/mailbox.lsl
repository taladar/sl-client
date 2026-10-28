// probe: throttle/email
// script: Mailbox
// prim: B
// distance: 3
// order: 1
// drive: none
// vary: after
//
// The Sender's addressee in prim B (see there). It answers the Sender's
// question on channel -7410 with its key, then asks for its mail with
// llGetNextEmail every two seconds, and prints each email that comes: the
// subject, whether the sender's address is the Sender's key at a domain and
// which domain, how many lines the body has, its first and last line, how
// many emails are still waiting, and how many seconds after the Sender's
// send it came (llGetUnixTime on both sides, so to the second).
string ID = "throttle/email";

integer CHANNEL = -7410;
key sender;
list sent_at;

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
        llListen(CHANNEL, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string text)
    {
        list parts = llParseString2List(text, ["|"], []);
        string what = llList2String(parts, 0);
        if (what == "where")
        {
            sender = id;
            llRegionSayTo(id, CHANNEL, "here|" + (string)llGetKey());
            llSetTimerEvent(2.0);
        }
        else if (what == "sent")
        {
            sent_at += [llList2String(parts, 1), llList2Integer(parts, 2)];
        }
    }

    timer()
    {
        llGetNextEmail("", "");
    }

    email(string time, string address, string subject, string message, integer left)
    {
        integer at = llSubStringIndex(address, "@");
        string local = llGetSubString(address, 0, at - 1);
        string domain = llGetSubString(address, at + 1, -1);
        integer index = llListFindList(sent_at, [subject]);
        string after = "unknown";
        if (index >= 0)
        {
            after = (string)(llGetUnixTime() - llList2Integer(sent_at, index + 1));
        }
        list lines = llParseStringKeepNulls(message, ["\n"], []);
        emit("email", " subject=" + q(subject)
            + " from_is_sender=" + (string)(local == (string)sender)
            + " domain=" + q(domain)
            + " lines=" + (string)llGetListLength(lines)
            + " first=" + q(llList2String(lines, 0))
            + " last=" + q(llList2String(lines, -1))
            + " left=" + (string)left
            + " after=" + after);
    }
}
