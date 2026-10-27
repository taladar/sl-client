// probe: prim/link
// script: Leader
// order: 2
// drive: answer
//
// Linking and the link-number functions, on each grid. Prim A holds this
// script, prim B the Child (see there). First llCreateLink with no
// permission, then the owner is asked for PERMISSION_CHANGE_LINKS and grants
// it; llCreateLink of NULL_KEY, of a key nothing has and of the owner; then
// prim B is linked as a child. Each step after that runs a second and a half
// apart, so that each prints the `changed` events its call raised in both
// prims. Linked: the link numbers, keys and names; llSetLinkPrimitiveParamsFast
// with each LINK_* constant and with the numbers 0, 1, 2 and 3, read back as
// the two prims' names; llGetLinkPrimitiveParams of the child (position,
// local position, rotation, size), of LINK_SET, LINK_ALL_OTHERS and a link
// that is not there, and PRIM_LINK_TARGET from one to the other; the child
// moved by PRIM_POS_LOCAL and PRIM_POSITION 5, 20 and 60 m from the root, and
// by its own llSetPos 15 m; llBreakLink(2), then linked again;
// llBreakLink(1) and llBreakLink of a link that is not there; llBreakAllLinks;
// last, prim A linked to prim B as B's child (parent FALSE), where this
// script then runs in a child, and llBreakAllLinks from there. What a grid
// says about a call is its debug-channel chat, which the runner notes beside
// the `try` record printed before it.
string ID = "prim/link";

key child;
integer found;
integer step;
vector home;
vector child_home;

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

string cm(vector v)
{
    return "<" + (string)(llRound(v.x * 100.0) / 100.0) + ","
        + (string)(llRound(v.y * 100.0) / 100.0) + ","
        + (string)(llRound(v.z * 100.0) / 100.0) + ">";
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

// Who a key is: this prim, the child, the owner, NULL_KEY or other.
string who(key k)
{
    if (k == llGetKey())
        return "self";
    if (k == child)
        return "child";
    if (k == llGetOwner())
        return "owner";
    if (k == NULL_KEY)
        return "null";
    return "other";
}

state_of(string label)
{
    emit("links", " label=" + label + " prims=" + (string)llGetNumberOfPrims()
        + " link=" + (string)llGetLinkNumber()
        + " key0=" + who(llGetLinkKey(0)) + " key1=" + who(llGetLinkKey(1))
        + " key2=" + who(llGetLinkKey(2)) + " key3=" + who(llGetLinkKey(3))
        + " name1=" + q(llGetLinkName(1)) + " name2=" + q(llGetLinkName(2)));
}

// Names both prims "Object", calls llSetLinkPrimitiveParamsFast on `link`
// with a new name, and prints both prims' names.
link_name(string label, integer link)
{
    llSetLinkPrimitiveParamsFast(LINK_SET, [PRIM_NAME, "Object"]);
    emit("try", " label=" + label);
    llSetLinkPrimitiveParamsFast(link, [PRIM_NAME, "named"]);
    emit("link-name", " label=" + label + " name1=" + q(llGetLinkName(1))
        + " name2=" + q(llGetLinkName(2)));
}

link_get(string label, integer link, list rules)
{
    emit("try", " label=" + label);
    list got = llGetLinkPrimitiveParams(link, rules);
    emit("link-get", " label=" + label + " count=" + (string)llGetListLength(got)
        + " got=" + q(llList2CSV(got)));
}

// The child's place, from the root's, after moving it by `rule`.
move_child(string label, integer rule, vector offset)
{
    llSetLinkPrimitiveParamsFast(2, [PRIM_POS_LOCAL, <2.0, 0.0, 0.0>]);
    emit("try", " label=" + label);
    if (rule == PRIM_POSITION)
    {
        llSetLinkPrimitiveParamsFast(2, [PRIM_POSITION, llGetPos() + offset]);
    }
    else
    {
        llSetLinkPrimitiveParamsFast(2, [rule, offset]);
    }
    vector local = llList2Vector(llGetLinkPrimitiveParams(2, [PRIM_POS_LOCAL]), 0);
    // A grid that takes PRIM_POSITION's region position for the offset puts
    // the child that far from the root: say so rather than print the region
    // position, which differs from grid to grid.
    string shown = cm(local);
    if (rule == PRIM_POSITION && llVecDist(local, llGetPos() + offset) < 0.01)
    {
        shown = "the-region-position";
    }
    emit("child-moved", " label=" + label + " local=" + shown);
}

next(float seconds)
{
    ++step;
    llSetTimerEvent(seconds);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        llListen(-7318, "", NULL_KEY, "");
        llRegionSay(-7317, "who");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) != llGetOwner() || found)
        {
            return;
        }
        found = TRUE;
        child = (key)message;
        child_home = llList2Vector(llGetObjectDetails(child, [OBJECT_POS]), 0);
        emit("child-found", " from_home=" + cm(child_home - home));
        emit("try", " label=link-without-permission");
        llCreateLink(child, TRUE);
        state_of("without-permission");
        emit("waiting", " drive=answer grant=128");
        llRequestPermissions(llGetOwner(), PERMISSION_CHANGE_LINKS);
    }

    run_time_permissions(integer perms)
    {
        emit("permissions", " got=" + (string)perms);
        emit("try", " label=link-null-key");
        llCreateLink(NULL_KEY, TRUE);
        emit("try", " label=link-no-such-key");
        llCreateLink("11111111-2222-3333-4444-555555555555", TRUE);
        emit("try", " label=link-owner");
        llCreateLink(llGetOwner(), TRUE);
        state_of("after-bad-links");
        emit("try", " label=link");
        llCreateLink(child, TRUE);
        step = 0;
        llSetTimerEvent(1.5);
    }

    changed(integer change)
    {
        emit("changed", " bits=" + names(change) + " link=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims());
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == 0)
        {
            state_of("linked");
            llMessageLinked(2, 0, "report", "linked");
            link_name("link-set", LINK_SET);
            link_name("link-all-others", LINK_ALL_OTHERS);
            link_name("link-all-children", LINK_ALL_CHILDREN);
            link_name("link-root", LINK_ROOT);
            link_name("link-this", LINK_THIS);
            link_name("link-0", 0);
            link_name("link-1", 1);
            link_name("link-2", 2);
            link_name("link-3", 3);
            llSetLinkPrimitiveParamsFast(LINK_SET, [PRIM_NAME, "Object"]);
            list got = llGetLinkPrimitiveParams(2, [PRIM_POSITION, PRIM_POS_LOCAL, PRIM_ROTATION,
                PRIM_ROT_LOCAL, PRIM_SIZE]);
            emit("link-get", " label=child-place from_root="
                + cm(llList2Vector(got, 0) - llGetPos()) + " local=" + cm(llList2Vector(got, 1))
                + " rot=" + q(llList2String(got, 2)) + " rot_local=" + q(llList2String(got, 3))
                + " size=" + cm(llList2Vector(got, 4)));
            link_get("link-get-set", LINK_SET, [PRIM_NAME]);
            link_get("link-get-others", LINK_ALL_OTHERS, [PRIM_NAME]);
            link_get("link-get-children", LINK_ALL_CHILDREN, [PRIM_NAME]);
            link_get("link-get-3", 3, [PRIM_NAME]);
            link_get("link-get-target", 1, [PRIM_NAME, PRIM_LINK_TARGET, 2, PRIM_DESC,
                PRIM_LINK_TARGET, 1, PRIM_DESC]);
            emit("try", " label=link-set-target");
            llSetLinkPrimitiveParamsFast(1, [PRIM_DESC, "root", PRIM_LINK_TARGET, 2,
                PRIM_DESC, "child"]);
            link_get("link-set-target", LINK_SET, [PRIM_DESC]);
            next(1.5);
        }
        else if (step == 1)
        {
            move_child("pos-local-5", PRIM_POS_LOCAL, <5.0, 0.0, 0.0>);
            move_child("pos-local-20", PRIM_POS_LOCAL, <20.0, 0.0, 0.0>);
            move_child("pos-local-60", PRIM_POS_LOCAL, <60.0, 0.0, 0.0>);
            move_child("position-5", PRIM_POSITION, <5.0, 0.0, 0.0>);
            move_child("position-20", PRIM_POSITION, <20.0, 0.0, 0.0>);
            move_child("position-60", PRIM_POSITION, <60.0, 0.0, 0.0>);
            llSetLinkPrimitiveParamsFast(2, [PRIM_POS_LOCAL, <2.0, 0.0, 0.0>]);
            llMessageLinked(2, 0, "setpos 15", "");
            next(2.0);
        }
        else if (step == 2)
        {
            llSetLinkPrimitiveParamsFast(2, [PRIM_POS_LOCAL, <2.0, 0.0, 0.0>]);
            emit("try", " label=break-2");
            llBreakLink(2);
            next(1.5);
        }
        else if (step == 3)
        {
            state_of("broken-2");
            emit("try", " label=relink");
            llCreateLink(child, TRUE);
            next(1.5);
        }
        else if (step == 4)
        {
            state_of("relinked");
            emit("try", " label=break-3");
            llBreakLink(3);
            emit("try", " label=break-1");
            llBreakLink(1);
            next(1.5);
        }
        else if (step == 5)
        {
            state_of("after-break-1");
            emit("try", " label=break-all");
            llBreakAllLinks();
            next(1.5);
        }
        else if (step == 6)
        {
            state_of("broken-all");
            emit("try", " label=break-all-single");
            llBreakAllLinks();
            emit("try", " label=link-as-child");
            llCreateLink(child, FALSE);
            next(1.5);
        }
        else if (step == 7)
        {
            state_of("as-child");
            llMessageLinked(LINK_ROOT, 0, "report", "as-child");
            emit("try", " label=break-all-from-child");
            llBreakAllLinks();
            next(1.5);
        }
        else if (step == 8)
        {
            state_of("end");
            llSetRegionPos(home);
            emit("end", "");
        }
    }
}
