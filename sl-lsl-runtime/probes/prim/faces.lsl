// probe: prim/faces
// drive: none
//
// The older per-face setters and getters, on each grid. llSetColor and
// llSetAlpha past their ranges and what llGetColor and llGetAlpha read, of
// one face and of ALL_SIDES (an average? a sum?) with the faces different;
// a face past the prim's faces and a negative one; llSetTexture by key, by a
// name not in the inventory, by a key no asset has and by NULL_KEY, and what
// llGetTexture reads; llScaleTexture, llOffsetTexture and llRotateTexture
// past their ranges, read back through PRIM_TEXTURE; llSetText past its
// length and alpha, read back through PRIM_TEXT; llGetNumberOfSides of each
// prim type, hollowed, cut, and both; and llSetLinkAlpha, llSetLinkColor and
// llSetLinkTexture of LINK_THIS. What a grid says about a call is its
// debug-channel chat, which the runner notes beside the `try` record printed
// before it.
string ID = "prim/faces";

string PLYWOOD = "89556747-24cb-43ed-920b-47caed15465f";
string BLANK = "5748decc-f629-461c-9a36-a35a221fe21f";

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

// A vector to four decimals.
string v4(vector v)
{
    return "<" + (string)(llRound(v.x * 10000.0) / 10000.0) + ","
        + (string)(llRound(v.y * 10000.0) / 10000.0) + ","
        + (string)(llRound(v.z * 10000.0) / 10000.0) + ">";
}

string f4(float f)
{
    return (string)(llRound(f * 10000.0) / 10000.0);
}

reset_faces()
{
    llSetPrimitiveParams([PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0,
        PRIM_TEXTURE, ALL_SIDES, BLANK, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0]);
}

color(string label, vector c, integer face)
{
    reset_faces();
    emit("try", " label=" + label);
    llSetColor(c, face);
    emit("color", " label=" + label + " face0=" + v4(llGetColor(0)) + " face1="
        + v4(llGetColor(1)) + " all=" + v4(llGetColor(ALL_SIDES)));
}

alpha(string label, float a, integer face)
{
    reset_faces();
    emit("try", " label=" + label);
    llSetAlpha(a, face);
    emit("alpha", " label=" + label + " face0=" + f4(llGetAlpha(0)) + " face1="
        + f4(llGetAlpha(1)) + " all=" + f4(llGetAlpha(ALL_SIDES)));
}

texture(string label, string t)
{
    reset_faces();
    emit("try", " label=" + label);
    llSetTexture(t, 0);
    string got = llGetTexture(0);
    string shown = got;
    if (got == PLYWOOD)
    {
        shown = "plywood";
    }
    else if (got == BLANK)
    {
        shown = "blank";
    }
    emit("texture", " label=" + label + " face0=" + q(shown));
}

placement(string label)
{
    list t = llGetPrimitiveParams([PRIM_TEXTURE, 0]);
    emit("placement", " label=" + label + " repeats=" + v4(llList2Vector(t, 1))
        + " offsets=" + v4(llList2Vector(t, 2)) + " rotation=" + f4(llList2Float(t, 3)));
}

sides(string label, list shape)
{
    llSetPrimitiveParams([PRIM_TYPE] + shape);
    emit("sides", " label=" + label + " sides=" + (string)llGetNumberOfSides());
}

default
{
    state_entry()
    {
        begin();
        reset_faces();
        emit("fresh", " sides=" + (string)llGetNumberOfSides()
            + " color=" + v4(llGetColor(ALL_SIDES)) + " alpha=" + f4(llGetAlpha(ALL_SIDES)));

        // Colour.
        color("one-face", <0.25, 0.5, 0.75>, 0);
        color("all-sides", <0.25, 0.5, 0.75>, ALL_SIDES);
        color("past-range", <2.0, -1.0, 0.5>, 0);
        color("face-6", <1.0, 0.0, 0.0>, 6);
        color("face-minus-2", <1.0, 0.0, 0.0>, -2);
        reset_faces();
        llSetColor(<1.0, 0.0, 0.0>, 0);
        llSetColor(<0.0, 0.0, 1.0>, 1);
        emit("color", " label=two-faces-differ all=" + v4(llGetColor(ALL_SIDES))
            + " face6=" + v4(llGetColor(6)) + " face-minus-2=" + v4(llGetColor(-2)));

        // Alpha.
        alpha("one-face", 0.25, 0);
        alpha("all-sides", 0.25, ALL_SIDES);
        alpha("past-range", 2.0, 0);
        alpha("negative", -1.0, 0);
        alpha("face-6", 0.5, 6);
        reset_faces();
        llSetAlpha(0.0, 0);
        llSetAlpha(0.5, 1);
        emit("alpha", " label=two-faces-differ all=" + f4(llGetAlpha(ALL_SIDES))
            + " face6=" + f4(llGetAlpha(6)));

        // Textures.
        texture("by-key", PLYWOOD);
        texture("missing-name", "no such texture");
        texture("unknown-key", "11111111-2222-3333-4444-555555555555");
        texture("null-key", NULL_KEY);
        texture("empty", "");
        reset_faces();
        emit("texture", " label=all-sides-fresh all=" + q(llGetTexture(ALL_SIDES))
            + " face6=" + q(llGetTexture(6)));

        // Repeats, offsets, rotation.
        reset_faces();
        llScaleTexture(2.0, 3.0, 0);
        placement("scale");
        llScaleTexture(200.0, -200.0, 0);
        placement("scale-past-range");
        llScaleTexture(0.0, 0.0, 0);
        placement("scale-zero");
        reset_faces();
        llOffsetTexture(0.25, -0.25, 0);
        placement("offset");
        llOffsetTexture(3.0, -3.0, 0);
        placement("offset-past-range");
        reset_faces();
        llRotateTexture(PI_BY_TWO, 0);
        placement("rotate");
        llRotateTexture(7.0, 0);
        placement("rotate-past-two-pi");
        llRotateTexture(-1.0, 0);
        placement("rotate-negative");
        reset_faces();

        // Hover text.
        llSetText("hello", <2.0, -1.0, 0.5>, 2.0);
        emit("text", " label=past-range got=" + q(llList2CSV(llGetPrimitiveParams([PRIM_TEXT]))));
        string long = "";
        integer i;
        for (i = 0; i < 30; ++i)
        {
            long += "0123456789";
        }
        llSetText(long, <1.0, 1.0, 1.0>, 1.0);
        emit("text", " label=300 length=" + (string)llStringLength(
            llList2String(llGetPrimitiveParams([PRIM_TEXT]), 0)));
        llSetText(llGetSubString(long, 0, 99) + "ééé" + long, <1.0, 1.0, 1.0>, 1.0);
        emit("text", " label=two-byte length=" + (string)llStringLength(
            llList2String(llGetPrimitiveParams([PRIM_TEXT]), 0)));
        llSetText("a\nb", <1.0, 1.0, 1.0>, -1.0);
        emit("text", " label=newline got=" + q(llList2CSV(llGetPrimitiveParams([PRIM_TEXT]))));
        llSetText("", ZERO_VECTOR, 0.0);

        // The faces of each shape.
        sides("box", [PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0, ZERO_VECTOR,
            <1.0, 1.0, 0.0>, ZERO_VECTOR]);
        sides("box-hollow", [PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.5,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
        sides("box-cut", [PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.25, 0.75, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
        sides("box-hollow-cut", [PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.25, 0.75, 0.0>, 0.5,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
        sides("box-tapered-to-point", [PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <0.0, 0.0, 0.0>, ZERO_VECTOR]);
        sides("cylinder", [PRIM_TYPE_CYLINDER, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
        sides("cylinder-hollow-cut", [PRIM_TYPE_CYLINDER, PRIM_HOLE_DEFAULT, <0.25, 0.75, 0.0>,
            0.5, ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
        sides("prism", [PRIM_TYPE_PRISM, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <0.0, 0.0, 0.0>, ZERO_VECTOR]);
        sides("sphere", [PRIM_TYPE_SPHERE, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <0.0, 1.0, 0.0>]);
        sides("sphere-hollow-cut-dimple", [PRIM_TYPE_SPHERE, PRIM_HOLE_DEFAULT,
            <0.25, 0.75, 0.0>, 0.5, ZERO_VECTOR, <0.2, 0.8, 0.0>]);
        sides("torus", [PRIM_TYPE_TORUS, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 0.25, 0.0>, ZERO_VECTOR, <0.0, 1.0, 0.0>, ZERO_VECTOR,
            0.0, 1.0, 0.0]);
        sides("tube", [PRIM_TYPE_TUBE, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 0.25, 0.0>, ZERO_VECTOR, <0.0, 1.0, 0.0>, ZERO_VECTOR,
            0.0, 1.0, 0.0]);
        sides("ring", [PRIM_TYPE_RING, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 0.25, 0.0>, ZERO_VECTOR, <0.0, 1.0, 0.0>, ZERO_VECTOR,
            0.0, 1.0, 0.0]);
        sides("sculpt", [PRIM_TYPE_SCULPT, PLYWOOD, PRIM_SCULPT_TYPE_SPHERE]);
        sides("box-again", [PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0, ZERO_VECTOR,
            <1.0, 1.0, 0.0>, ZERO_VECTOR]);

        // The link setters of LINK_THIS.
        reset_faces();
        llSetLinkColor(LINK_THIS, <0.0, 1.0, 0.0>, 0);
        llSetLinkAlpha(LINK_THIS, 0.5, 0);
        llSetLinkTexture(LINK_THIS, PLYWOOD, 0);
        emit("link", " label=this color=" + v4(llGetColor(0)) + " alpha=" + f4(llGetAlpha(0))
            + " plywood=" + (string)(llGetTexture(0) == PLYWOOD));
        reset_faces();
        llSetLinkColor(2, <0.0, 1.0, 0.0>, 0);
        llSetLinkColor(0, <0.0, 0.0, 1.0>, 1);
        emit("link", " label=two-and-zero face0=" + v4(llGetColor(0)) + " face1="
            + v4(llGetColor(1)));
        reset_faces();

        emit("end", "");
    }
}
