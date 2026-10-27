// probe: inventory/listing
// script: Probe
// order: 8
// drive: none
//
// Reading the prim's inventory. Prim A holds seven notecards whose names
// differ in case, digits, an underscore and an accent (alpha, Beta, _under,
// 10, 9, Zed, élan), an object (Box), this script and a helper (aaa Helper).
// For every INVENTORY_* type (material as 57, which OpenSim does not name),
// and a type that is none: the count and the names in index order (out of
// range is prim B's, see there); llGetInventoryType of each kind, of a name
// in the wrong case and of none; llGetInventoryKey, llGetInventoryCreator and
// the five permission masks of a notecard, the object, a script and a name
// that is not there; llSetInventoryPermMask of the next-owner mask, read
// back. Then the scripts: llGetScriptName, llGetScriptState of the helper, of
// this script and of none, stopping the helper and pinging it,
// llResetOtherScript of the stopped helper, starting it again, resetting it
// running, and resetting a script that is not there; llScriptDanger here.
// Last llRemoveInventory of a notecard, of a name that is not there and of
// the object, with the `changed` each raises and the counts after. What a
// grid says about a call is its debug-channel chat, which the runner notes
// beside the `try` record printed before it.
string ID = "inventory/listing";

integer step;

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

try(string label)
{
    emit("try", " label=" + label);
}

listing(string when)
{
    list types = [INVENTORY_ALL, INVENTORY_TEXTURE, INVENTORY_SOUND, INVENTORY_LANDMARK,
        INVENTORY_CLOTHING, INVENTORY_OBJECT, INVENTORY_NOTECARD, INVENTORY_SCRIPT,
        INVENTORY_BODYPART, INVENTORY_ANIMATION, INVENTORY_GESTURE, INVENTORY_SETTING,
        57, 4, 99, -2];
    integer t;
    for (t = 0; t < llGetListLength(types); ++t)
    {
        integer type = llList2Integer(types, t);
        integer n = llGetInventoryNumber(type);
        list names;
        integer i;
        for (i = 0; i < n; ++i)
        {
            names += llGetInventoryName(type, i);
        }
        emit("count", " when=" + when + " type=" + (string)type + " n=" + (string)n
            + " names=" + q(llDumpList2String(names, "|")));
    }
}

string key_note(key k)
{
    if (k == NULL_KEY) return "null";
    if (k == llGetOwner()) return "owner";
    if ((string)k == "") return "empty";
    return "other";
}

item(string name)
{
    string masks;
    list which = [MASK_BASE, MASK_OWNER, MASK_GROUP, MASK_EVERYONE, MASK_NEXT];
    integer m;
    for (m = 0; m < 5; ++m)
    {
        if (m) masks += ",";
        masks += (string)llGetInventoryPermMask(name, llList2Integer(which, m));
    }
    emit("item", " name=" + q(name) + " type=" + (string)llGetInventoryType(name)
        + " key=" + key_note(llGetInventoryKey(name))
        + " creator=" + key_note(llGetInventoryCreator(name))
        + " masks=" + masks);
}

default
{
    state_entry()
    {
        begin();
        listing("first");
        list names = ["alpha", "ALPHA", "Box", "Probe", "aaa Helper", "élan", "nope", ""];
        integer i;
        for (i = 0; i < llGetListLength(names); ++i)
        {
            try("item-" + (string)i);
            item(llList2String(names, i));
        }
        try("set-perm-mask");
        llSetInventoryPermMask("alpha", MASK_NEXT, PERM_COPY | PERM_TRANSFER);
        item("alpha");
        emit("script", " name=" + q(llGetScriptName())
            + " helper=" + (string)llGetScriptState("aaa Helper")
            + " self=" + (string)llGetScriptState(llGetScriptName())
            + " danger=" + (string)llScriptDanger(llGetPos()));
        try("state-of-none");
        emit("script", " case=state-of-none value=" + (string)llGetScriptState("nope"));
        llSetTimerEvent(1.0);
    }

    timer()
    {
        // XEngine runs a timer event queued before the timer was stopped.
        if (step > 8) return;
        ++step;
        if (step == 1)
        {
            try("stop-helper");
            llSetScriptState("aaa Helper", FALSE);
            emit("script", " case=stopped value=" + (string)llGetScriptState("aaa Helper"));
            llMessageLinked(LINK_THIS, 1, "ping", NULL_KEY);
        }
        else if (step == 2)
        {
            try("reset-stopped-helper");
            llResetOtherScript("aaa Helper");
            emit("script", " case=reset-while-stopped value=" + (string)llGetScriptState("aaa Helper"));
            llMessageLinked(LINK_THIS, 2, "ping", NULL_KEY);
        }
        else if (step == 3)
        {
            try("start-helper");
            llSetScriptState("aaa Helper", TRUE);
            emit("script", " case=started value=" + (string)llGetScriptState("aaa Helper"));
        }
        else if (step == 4)
        {
            llMessageLinked(LINK_THIS, 4, "ping", NULL_KEY);
            try("reset-running-helper");
            llResetOtherScript("aaa Helper");
        }
        else if (step == 5)
        {
            llMessageLinked(LINK_THIS, 5, "ping", NULL_KEY);
            try("reset-none");
            llResetOtherScript("nope");
            try("set-state-none");
            llSetScriptState("nope", FALSE);
        }
        else if (step == 6)
        {
            try("remove-notecard");
            llRemoveInventory("Zed");
            emit("removed", " name=Zed n=" + (string)llGetInventoryNumber(INVENTORY_NOTECARD));
        }
        else if (step == 7)
        {
            try("remove-none");
            llRemoveInventory("nope");
            emit("removed", " name=nope n=" + (string)llGetInventoryNumber(INVENTORY_ALL));
        }
        else if (step == 8)
        {
            try("remove-object");
            llRemoveInventory("Box");
            emit("removed", " name=Box n=" + (string)llGetInventoryNumber(INVENTORY_OBJECT));
        }
        else
        {
            llSetTimerEvent(0.0);
            listing("last");
            emit("end", "");
        }
    }

    changed(integer change)
    {
        emit("changed", " change=" + (string)change + " step=" + (string)step);
    }
}
