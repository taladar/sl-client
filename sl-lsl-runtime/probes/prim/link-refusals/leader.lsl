// probe: prim/link-refusals
// script: Leader
// order: 4
// drive: answer, sit, stand
//
// What `prim/link` left open: what each grid says when a link or a delink is
// refused, and what becomes of an avatar sitting on the set when a script
// delinks. Prim A holds this script and the Unpermitted script, which holds no
// permission at all; prim B (the Child, two metres east) and prim C (the Far
// prim, seventy metres above) answer "who" with their keys. The owner grants
// this script PERMISSION_CHANGE_LINKS; it links prim C, further than Second
// Life's linksets reach, then prim B. Linked, it asks the Unpermitted script
// to call llBreakLink(2), llBreakLink(1) and llBreakAllLinks, each without the
// permission, and reads the set after. Then the other avatar sits on prim A
// and this script calls llBreakLink(2); whoever still sits (or, if the call
// stood it up, the avatar sitting down again) stays while prim B is linked
// again, llBreakLink is called with the sitter's own link number and, last,
// llBreakAllLinks; whoever still sits then stands up. Each step runs a second
// or two after the one before, so that each prints the `changed` events its
// call raised in prims A and B. What a grid says about a call is its
// debug-channel chat, which the runner notes beside the `try` record printed
// before it.
string ID = "prim/link-refusals";

key child;
key far;
integer have_child;
integer have_far;
integer step;
integer awaiting_sit;
integer awaiting_stand;

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

string names(integer b)
{
    list NAMES = ["INVENTORY", "COLOR", "SHAPE", "SCALE", "TEXTURE", "LINK", "ALLOWED_DROP",
        "OWNER", "REGION", "TELEPORT", "REGION_START", "MEDIA", "RENDER_MATERIAL"];
    string out = "";
    integer i;
    for (i = 0; i < 13; ++i)
    {
        if (b & (1 << i))
        {
            if (out != "")
            {
                out += "|";
            }
            out += llList2String(NAMES, i);
        }
    }
    integer rest = b & ~0x1FFF;
    if (rest)
    {
        if (out != "")
        {
            out += "|";
        }
        out += (string)rest;
    }
    if (out == "")
    {
        out = "none";
    }
    return out;
}

// Who a key is: this prim, prim B, prim C, the owner, an avatar (the other
// one, the only one that sits), NULL_KEY or other.
string who(key k)
{
    if (k == llGetKey())
        return "self";
    if (k == child)
        return "child";
    if (k == far)
        return "far";
    if (k == llGetOwner())
        return "owner";
    if (k == NULL_KEY)
        return "null";
    if (llGetAgentSize(k) != ZERO_VECTOR)
        return "avatar";
    return "other";
}

// Whether an avatar sits on the set: the link after the last prim's is one.
integer seated()
{
    integer prims = llGetNumberOfPrims();
    return llGetAgentSize(llGetLinkKey(prims)) != ZERO_VECTOR;
}

state_of(string label)
{
    emit("links", " label=" + label + " prims=" + (string)llGetNumberOfPrims()
        + " link=" + (string)llGetLinkNumber()
        + " seated=" + (string)seated()
        + " key1=" + who(llGetLinkKey(1)) + " key2=" + who(llGetLinkKey(2))
        + " key3=" + who(llGetLinkKey(3)) + " key4=" + who(llGetLinkKey(4)));
}

next(float seconds)
{
    ++step;
    llSetTimerEvent(seconds);
}

// Asks for the other avatar to sit on prim A; `changed` goes on once it does.
sit()
{
    awaiting_sit = TRUE;
    emit("waiting", " drive=sit by=other");
}

default
{
    state_entry()
    {
        begin();
        llListen(-7318, "", NULL_KEY, "");
        // Asked again every two seconds until both have answered: a prim
        // whose script is not listening yet hears nothing.
        step = -1;
        llRegionSay(-7317, "who");
        llSetTimerEvent(2.0);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) != llGetOwner())
        {
            return;
        }
        string which = llGetSubString(message, 0, 0);
        key k = (key)llGetSubString(message, 2, -1);
        if (which == "B" && !have_child)
        {
            have_child = TRUE;
            child = k;
        }
        else if (which == "C" && !have_far)
        {
            have_far = TRUE;
            far = k;
        }
        else
        {
            return;
        }
        if (have_child && have_far)
        {
            llSetTimerEvent(0.0);
            step = 0;
            emit("found", "");
            emit("waiting", " drive=answer grant=128");
            llRequestPermissions(llGetOwner(), PERMISSION_CHANGE_LINKS);
        }
    }

    run_time_permissions(integer perms)
    {
        emit("permissions", " got=" + (string)perms);
        emit("try", " label=link-too-far");
        llCreateLink(far, TRUE);
        state_of("after-too-far");
        emit("try", " label=link");
        llCreateLink(child, TRUE);
        step = 0;
        llSetTimerEvent(1.5);
    }

    changed(integer change)
    {
        emit("changed", " bits=" + names(change) + " link=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims() + " seated=" + (string)seated());
        if (awaiting_sit && seated())
        {
            awaiting_sit = FALSE;
            llSetTimerEvent(1.0);
        }
        else if (awaiting_stand && !seated())
        {
            awaiting_stand = FALSE;
            llSetTimerEvent(1.0);
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == -1)
        {
            llRegionSay(-7317, "who");
            llSetTimerEvent(2.0);
        }
        else if (step == 0)
        {
            state_of("linked");
            llMessageLinked(LINK_THIS, 0, "unpermitted", "");
            next(2.0);
        }
        else if (step == 1)
        {
            state_of("after-unpermitted");
            ++step;
            sit();
        }
        else if (step == 2)
        {
            state_of("seated");
            emit("try", " label=break-2-seated");
            llBreakLink(2);
            next(2.0);
        }
        else if (step == 3)
        {
            state_of("after-break-2-seated");
            ++step;
            if (seated())
            {
                llSetTimerEvent(0.1);
            }
            else
            {
                sit();
            }
        }
        else if (step == 4)
        {
            state_of("before-relink-seated");
            emit("try", " label=relink-seated");
            llCreateLink(child, TRUE);
            next(2.0);
        }
        else if (step == 5)
        {
            state_of("relinked-seated");
            integer sitter = llGetNumberOfPrims();
            emit("try", " label=break-sitter link=" + (string)sitter);
            llBreakLink(sitter);
            next(2.0);
        }
        else if (step == 6)
        {
            state_of("after-break-sitter");
            ++step;
            if (seated())
            {
                llSetTimerEvent(0.1);
            }
            else
            {
                sit();
            }
        }
        else if (step == 7)
        {
            state_of("before-break-all-seated");
            emit("try", " label=break-all-seated");
            llBreakAllLinks();
            next(2.0);
        }
        else if (step == 8)
        {
            state_of("after-break-all-seated");
            ++step;
            if (seated())
            {
                awaiting_stand = TRUE;
                emit("waiting", " drive=stand by=other");
            }
            else
            {
                llSetTimerEvent(0.1);
            }
        }
        else if (step == 9)
        {
            state_of("end");
            emit("end", "");
        }
    }
}
