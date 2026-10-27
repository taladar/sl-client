// probe: prim/params-rules
// script: Probe
// order: 4
// drive: none
//
// How llGetPrimitiveParams and llSetPrimitiveParams treat a rule list as a
// whole, on each grid. The order of what a get returns for several rules,
// the same rule twice and ALL_SIDES (one entry per face, in face order);
// face numbers past the prim's faces and negative ones, in a get and in a
// set; an unknown rule code in a get and in a set, and whether the rules
// after it still run; a rule with a value of the wrong type, one with too
// few values, an integer where a float goes and a float where an integer
// goes; PRIM_LINK_TARGET on a single prim (link 0, 1 and 2, LINK_THIS);
// an empty list; and what llSetLinkPrimitiveParamsFast and
// llGetLinkPrimitiveParams do with the link numbers of a single prim.
// What a grid says about a rule is its debug-channel chat, which the runner
// notes beside the `try` record printed before each call. A rule code given
// as a string and a get of face 100 and of face -2 are tried by the scripts
// in prim B (see there), one each, because OpenSim throws on a face number
// it cannot index.
string ID = "prim/params-rules";

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

// The types of a list, one letter each: i f s k v r.
string types(list l)
{
    string out = "";
    integer n = llGetListLength(l);
    integer i;
    for (i = 0; i < n; ++i)
    {
        out += llGetSubString("?ifskvr", llGetListEntryType(l, i), llGetListEntryType(l, i));
    }
    return out;
}

get(string label, list rules)
{
    emit("try", " label=" + label);
    list got = llGetPrimitiveParams(rules);
    emit("get", " label=" + label + " count=" + (string)llGetListLength(got)
        + " types=" + types(got) + " got=" + q(llList2CSV(got)));
}

link_get(string label, integer link, list rules)
{
    emit("try", " label=" + label);
    list got = llGetLinkPrimitiveParams(link, rules);
    emit("get", " label=" + label + " count=" + (string)llGetListLength(got)
        + " types=" + types(got) + " got=" + q(llList2CSV(got)));
}

after(string label)
{
    list got = llGetPrimitiveParams([PRIM_NAME, PRIM_GLOW, 0, PRIM_COLOR, 0, PRIM_GLOW, 5]);
    emit("set", " label=" + label + " name=" + q(llList2String(got, 0))
        + " glow0=" + (string)llList2Float(got, 1)
        + " color0=" + q(llList2String(got, 2))
        + " glow5=" + (string)llList2Float(got, 4));
}

// Sets `rules`, then prints the name, the glow of face 0 and the colour of
// face 0: what the legs below change, so that whether the rules after an
// error ran can be read.
set(string label, list rules)
{
    llSetPrimitiveParams([PRIM_NAME, "Object", PRIM_GLOW, ALL_SIDES, 0.0,
        PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);
    emit("try", " label=" + label);
    llSetPrimitiveParams(rules);
    after(label);
}

link_set(string label, integer link, list rules)
{
    llSetPrimitiveParams([PRIM_NAME, "Object", PRIM_GLOW, ALL_SIDES, 0.0,
        PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);
    emit("try", " label=" + label);
    llSetLinkPrimitiveParamsFast(link, rules);
    after(label);
}

default
{
    state_entry()
    {
        begin();
        emit("faces", " sides=" + (string)llGetNumberOfSides());

        // Order and repetition.
        get("order", [PRIM_SIZE, PRIM_NAME, PRIM_GLOW, 0, PRIM_MATERIAL]);
        get("order-reversed", [PRIM_MATERIAL, PRIM_GLOW, 0, PRIM_NAME, PRIM_SIZE]);
        get("same-twice", [PRIM_NAME, PRIM_NAME]);
        get("all-sides-glow", [PRIM_GLOW, ALL_SIDES]);
        get("all-sides-color", [PRIM_COLOR, ALL_SIDES]);
        get("all-sides-then-name", [PRIM_FULLBRIGHT, ALL_SIDES, PRIM_NAME]);
        get("empty", []);

        // Faces out of range.
        get("face-6", [PRIM_GLOW, 6, PRIM_NAME]);
        set("set-face-6", [PRIM_GLOW, 6, 0.5, PRIM_NAME, "after"]);
        set("set-face-minus-2", [PRIM_GLOW, -2, 0.5, PRIM_NAME, "after"]);

        // Unknown codes.
        get("unknown-code", [9999, PRIM_NAME]);
        get("unknown-negative-code", [-5, PRIM_NAME]);
        set("set-unknown-code", [9999, PRIM_NAME, "after"]);
        set("set-unknown-then-glow", [9999, 1, PRIM_GLOW, 0, 0.5]);

        // Values of the wrong type, too few values.
        set("glow-as-string", [PRIM_GLOW, 0, "0.5", PRIM_NAME, "after"]);
        set("glow-as-integer", [PRIM_GLOW, 0, 1, PRIM_NAME, "after"]);
        set("glow-as-vector", [PRIM_GLOW, 0, <0.5, 0.0, 0.0>, PRIM_NAME, "after"]);
        set("face-as-float", [PRIM_GLOW, 0.0, 0.5, PRIM_NAME, "after"]);
        set("color-as-string", [PRIM_COLOR, 0, "<1,0,0>", 1.0, PRIM_NAME, "after"]);
        set("name-as-integer", [PRIM_NAME, 7, PRIM_GLOW, 0, 0.5]);
        set("name-as-key", [PRIM_NAME, (key)"11111111-2222-3333-4444-555555555555",
            PRIM_GLOW, 0, 0.5]);
        set("glow-too-few", [PRIM_NAME, "before", PRIM_GLOW, 0]);
        set("color-too-few", [PRIM_NAME, "before", PRIM_COLOR, 0, <1.0, 0.0, 0.0>]);
        set("a-good-rule-before-a-bad", [PRIM_NAME, "before", PRIM_GLOW, 0, "x",
            PRIM_COLOR, 0, <1.0, 0.0, 0.0>, 1.0]);

        // PRIM_LINK_TARGET on a single prim.
        get("link-target-1", [PRIM_LINK_TARGET, 1, PRIM_NAME]);
        get("link-target-0", [PRIM_LINK_TARGET, 0, PRIM_NAME]);
        get("link-target-2", [PRIM_NAME, PRIM_LINK_TARGET, 2, PRIM_NAME]);
        get("link-target-this", [PRIM_LINK_TARGET, LINK_THIS, PRIM_NAME]);
        get("link-target-set", [PRIM_LINK_TARGET, LINK_SET, PRIM_NAME]);
        get("link-target-last", [PRIM_NAME, PRIM_LINK_TARGET]);
        set("set-link-target-0", [PRIM_LINK_TARGET, 0, PRIM_NAME, "after"]);
        set("set-link-target-2", [PRIM_LINK_TARGET, 2, PRIM_NAME, "after"]);
        set("set-link-target-2-then-1", [PRIM_LINK_TARGET, 2, PRIM_GLOW, 0, 0.5,
            PRIM_LINK_TARGET, 1, PRIM_NAME, "after"]);

        // The link functions on a single prim.
        link_get("link-get-0", 0, [PRIM_NAME]);
        link_get("link-get-1", 1, [PRIM_NAME]);
        link_get("link-get-2", 2, [PRIM_NAME]);
        link_get("link-get-this", LINK_THIS, [PRIM_NAME]);
        link_get("link-get-set", LINK_SET, [PRIM_NAME]);
        link_get("link-get-root", LINK_ROOT, [PRIM_NAME]);
        link_set("link-set-0", 0, [PRIM_NAME, "after"]);
        link_set("link-set-1", 1, [PRIM_NAME, "after"]);
        link_set("link-set-2", 2, [PRIM_NAME, "after"]);
        link_set("link-set-set", LINK_SET, [PRIM_NAME, "after"]);
        link_set("link-set-others", LINK_ALL_OTHERS, [PRIM_NAME, "after"]);
        link_set("link-set-children", LINK_ALL_CHILDREN, [PRIM_NAME, "after"]);
        link_set("link-set-root", LINK_ROOT, [PRIM_NAME, "after"]);

        llSetPrimitiveParams([PRIM_NAME, "Object", PRIM_GLOW, ALL_SIDES, 0.0,
            PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);
        emit("end", "");
    }
}
