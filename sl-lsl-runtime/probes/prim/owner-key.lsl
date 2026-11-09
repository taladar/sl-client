// probe: prim/owner-key
// drive: none
//
// What llGetOwnerKey answers, on each grid: for this prim, its key in upper
// case, the owner, another avatar in the region (the first of
// llGetAgentList that is not the owner, when there is one), NULL_KEY, a key
// nothing in the region has, text that is no key and the empty string. Keys
// print as roles: this prim's, the owner's, NULL_KEY, empty, the key asked
// about itself (`same`), or another.
string ID = "prim/owner-key";

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

// Who a key is, as a role, beside the key it was asked about.
string who(string k, string asked)
{
    if (k == (string)llGetKey())
        return "self";
    if (k == (string)llGetOwner())
        return "owner";
    if (k == NULL_KEY)
        return "null";
    if (k == "")
        return "empty";
    if (k == asked)
        return "same";
    return "other";
}

ask(string label, string id)
{
    key got = llGetOwnerKey(id);
    emit("owner-key", " label=" + label + " got=" + who((string)got, id)
        + " valid=" + (string)(got != NULL_KEY && got != ""));
}

default
{
    state_entry()
    {
        begin();
        ask("self", (string)llGetKey());
        ask("self-upper", llToUpper((string)llGetKey()));
        ask("owner", (string)llGetOwner());
        list agents = llGetAgentList(AGENT_LIST_REGION, []);
        integer i;
        string other = "";
        for (i = 0; i < llGetListLength(agents) && other == ""; ++i)
        {
            if (llList2Key(agents, i) != llGetOwner())
                other = llList2String(agents, i);
        }
        if (other != "")
            ask("stranger", other);
        else
            emit("no-stranger", "");
        ask("null", NULL_KEY);
        ask("nothing", "0b8c4a5e-3f1d-4e2a-9c7b-6d5e4f3a2b1c");
        ask("junk", "not a key");
        ask("empty", "");
        emit("end", "");
    }
}
