// probe: data/linkset-data
// script: Watcher
// order: 2
// drive: none
// vary: got
//
// The Leader's neighbour in prim A (see there): it says it is ready by link
// message and keeps every linkset_data event it gets until the Leader asks
// it to print them — except while the Leader fills the store, which the
// Leader tells it by link message before and after, where it only counts
// them: how many it gets is how far it keeps up with the Leader's writes,
// which is timing (`got`).
string ID = "data/linkset-data";

list seen;
integer filling;
integer fill_count;
list ACTIONS = ["reset", "update", "delete", "multidelete"];

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

// A string as a record shows it: quoted, or only its length when it is
// longer than 40 characters.
string shorten(string s)
{
    if (llStringLength(s) > 40)
    {
        return "length:" + (string)llStringLength(s);
    }
    return q(s);
}

string action(integer a)
{
    if (a >= 0 && a < llGetListLength(ACTIONS))
    {
        return llList2String(ACTIONS, a);
    }
    return (string)a;
}

default
{
    state_entry()
    {
        llMessageLinked(LINK_THIS, 1, "ready", "");
    }

    linkset_data(integer act, string name, string value)
    {
        if (filling)
        {
            fill_count = fill_count + 1;
            return;
        }
        seen += [" action=" + action(act) + " name=" + shorten(name)
            + " value=" + shorten(value)];
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num == 3)
        {
            filling = TRUE;
        }
        else if (num == 4)
        {
            filling = FALSE;
        }
        else if (num == 2 && text == "report")
        {
            integer i;
            for (i = 0; i < llGetListLength(seen); ++i)
            {
                emit("watcher-event", " n=" + (string)i + llList2String(seen, i));
            }
            emit("watcher-fill-events", " got=" + (string)fill_count);
        }
    }
}
