// probe: prim/params
// script: Probe
// order: 2
// drive: none
//
// What llGetPrimitiveParams reads back after llSetPrimitiveParams, rule by
// rule, on each grid: each `rt` record sets one rule (or a few values of it)
// and prints what the matching get returns, as CSV. Shapes with values inside
// and outside their documented ranges (hollow, cut, twist, taper, shear,
// dimple, hole size, radius offset, revolutions, skew, hole shapes, a sculpt);
// colour and alpha past 1 and below 0; a texture by key, with repeats,
// offsets and a rotation past their ranges; bump and shiny, fullbright,
// glow, texgen, material, a point light and a flexible path past their
// ranges; hover text past its length and alpha; slice, name and description
// past their lengths, phantom, position and rotation, alpha mode, normal and
// specular maps, the sit target, the click action, the sit flags and the
// newer rules by number (OpenSim has no constants for them, so a script that
// named them would not compile there). The shape and the faces are put back
// as they were after each group. Setting PRIM_CAST_SHADOWS is tried by the
// script in prim B (see there), because XEngine never comes back from it,
// and is left out there.
// Rule order, ALL_SIDES and errors are `prim/params-rules`.
string ID = "prim/params";

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

// Sets `set`, then prints what `get` reads.
rt(string label, list set, list get)
{
    emit("try", " label=" + label);
    llSetPrimitiveParams(set);
    emit("rt", " label=" + label + " got=" + q(llList2CSV(llGetPrimitiveParams(get))));
}

// Prints what `get` reads, setting nothing.
read(string label, list get)
{
    emit("read", " label=" + label + " got=" + q(llList2CSV(llGetPrimitiveParams(get))));
}

box()
{
    llSetPrimitiveParams([PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
        ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
}

default
{
    state_entry()
    {
        begin();

        // What a fresh prim reads, rule by rule.
        read("fresh-type", [PRIM_TYPE]);
        read("fresh-color", [PRIM_COLOR, 0]);
        read("fresh-texture", [PRIM_TEXTURE, 0]);
        read("fresh-bump-shiny", [PRIM_BUMP_SHINY, 0]);
        read("fresh-fullbright", [PRIM_FULLBRIGHT, 0]);
        read("fresh-glow", [PRIM_GLOW, 0]);
        read("fresh-texgen", [PRIM_TEXGEN, 0]);
        read("fresh-material", [PRIM_MATERIAL]);
        read("fresh-light", [PRIM_POINT_LIGHT]);
        read("fresh-flexible", [PRIM_FLEXIBLE]);
        read("fresh-text", [PRIM_TEXT]);
        read("fresh-slice", [PRIM_SLICE]);
        read("fresh-name-desc", [PRIM_NAME, PRIM_DESC]);
        read("fresh-alpha-mode", [PRIM_ALPHA_MODE, 0]);
        read("fresh-normal", [PRIM_NORMAL, 0]);
        read("fresh-specular", [PRIM_SPECULAR, 0]);
        read("fresh-physics-shape", [PRIM_PHYSICS_SHAPE_TYPE]);
        read("fresh-omega", [PRIM_OMEGA]);
        read("fresh-sit-target", [41]);
        read("fresh-allow-unsit", [39]);
        read("fresh-scripted-sit-only", [40]);
        read("fresh-click-action", [43]);
        read("fresh-projector", [42]);
        read("fresh-reflection-probe", [44]);
        read("fresh-render-material", [49, 0]);
        read("fresh-sit-flags", [50]);
        read("fresh-cast-shadows", [24]);
        read("fresh-temp-on-rez", [PRIM_TEMP_ON_REZ]);

        // Shapes.
        rt("box-in-range", [PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_SQUARE, <0.25, 0.75, 0.0>, 0.5,
            <0.0, 0.5, 0.0>, <0.5, 1.5, 0.0>, <0.25, -0.25, 0.0>], [PRIM_TYPE]);
        rt("box-past-range", [PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <-0.5, 1.5, 0.0>, 1.0,
            <-2.0, 2.0, 0.0>, <3.0, -1.0, 0.0>, <1.0, -1.0, 0.0>], [PRIM_TYPE]);
        rt("box-cut-reversed", [PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.8, 0.2, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        rt("box-cut-tiny", [PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.5, 0.51, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        rt("box-hollow-0.99", [PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.99,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        rt("box-hole-unknown", [PRIM_TYPE, PRIM_TYPE_BOX, 0x40, <0.0, 1.0, 0.0>, 0.5,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        rt("cylinder", [PRIM_TYPE, PRIM_TYPE_CYLINDER, PRIM_HOLE_TRIANGLE, <0.0, 1.0, 0.0>, 0.3,
            <0.0, 0.25, 0.0>, <1.0, 0.5, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        rt("prism", [PRIM_TYPE, PRIM_TYPE_PRISM, PRIM_HOLE_CIRCLE, <0.0, 1.0, 0.0>, 0.3,
            ZERO_VECTOR, <0.0, 0.0, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        rt("sphere-dimple", [PRIM_TYPE, PRIM_TYPE_SPHERE, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <0.2, 0.7, 0.0>], [PRIM_TYPE]);
        rt("sphere-dimple-reversed", [PRIM_TYPE, PRIM_TYPE_SPHERE, PRIM_HOLE_DEFAULT,
            <0.0, 1.0, 0.0>, 0.0, ZERO_VECTOR, <0.9, 0.1, 0.0>], [PRIM_TYPE]);
        rt("torus-in-range", [PRIM_TYPE, PRIM_TYPE_TORUS, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 0.25, 0.0>, <0.0, 0.0, 0.0>, <0.0, 1.0, 0.0>, <0.0, 0.0, 0.0>,
            0.0, 2.0, 0.0], [PRIM_TYPE]);
        rt("torus-past-range", [PRIM_TYPE, PRIM_TYPE_TORUS, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <2.0, 0.01, 0.0>, <1.0, 0.0, 0.0>, <0.0, 1.0, 0.0>, <2.0, 2.0, 0.0>,
            2.0, 5.0, 2.0], [PRIM_TYPE]);
        rt("tube", [PRIM_TYPE, PRIM_TYPE_TUBE, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 0.5, 0.0>, ZERO_VECTOR, <0.0, 1.0, 0.0>, ZERO_VECTOR,
            0.0, 1.0, 0.0], [PRIM_TYPE]);
        rt("ring", [PRIM_TYPE, PRIM_TYPE_RING, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 0.5, 0.0>, ZERO_VECTOR, <0.0, 1.0, 0.0>, ZERO_VECTOR,
            0.0, 1.0, 0.0], [PRIM_TYPE]);
        rt("sculpt-sphere", [PRIM_TYPE, PRIM_TYPE_SCULPT, PLYWOOD, PRIM_SCULPT_TYPE_SPHERE],
            [PRIM_TYPE]);
        rt("sculpt-flags", [PRIM_TYPE, PRIM_TYPE_SCULPT, PLYWOOD,
            PRIM_SCULPT_TYPE_TORUS | PRIM_SCULPT_FLAG_INVERT | PRIM_SCULPT_FLAG_MIRROR],
            [PRIM_TYPE]);
        rt("sculpt-type-unknown", [PRIM_TYPE, PRIM_TYPE_SCULPT, PLYWOOD, 6], [PRIM_TYPE]);
        rt("sculpt-not-a-key", [PRIM_TYPE, PRIM_TYPE_SCULPT, "nothing", PRIM_SCULPT_TYPE_SPHERE],
            [PRIM_TYPE]);
        rt("type-unknown", [PRIM_TYPE, 9, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.0,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR], [PRIM_TYPE]);
        box();
        read("box-again", [PRIM_TYPE]);

        // Colour and alpha.
        rt("color", [PRIM_COLOR, 0, <0.25, 0.5, 0.75>, 0.5], [PRIM_COLOR, 0]);
        rt("color-past-range", [PRIM_COLOR, 0, <2.0, -1.0, 0.5>, 1.5], [PRIM_COLOR, 0]);
        rt("color-alpha-negative", [PRIM_COLOR, 0, <1.0, 1.0, 1.0>, -0.5], [PRIM_COLOR, 0]);
        rt("color-fine", [PRIM_COLOR, 0, <0.001, 0.333, 0.999>, 0.333], [PRIM_COLOR, 0]);
        llSetPrimitiveParams([PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);

        // Textures.
        rt("texture", [PRIM_TEXTURE, 0, PLYWOOD, <2.0, 3.0, 0.0>, <0.25, -0.25, 0.0>, PI_BY_TWO],
            [PRIM_TEXTURE, 0]);
        rt("texture-past-range", [PRIM_TEXTURE, 0, PLYWOOD, <200.0, -200.0, 0.0>, <3.0, -3.0, 0.0>,
            7.0], [PRIM_TEXTURE, 0]);
        rt("texture-zero-repeats", [PRIM_TEXTURE, 0, PLYWOOD, <0.0, 0.0, 0.0>, ZERO_VECTOR, 0.0],
            [PRIM_TEXTURE, 0]);
        rt("texture-unknown-key", [PRIM_TEXTURE, 0, "11111111-2222-3333-4444-555555555555",
            <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0], [PRIM_TEXTURE, 0]);
        rt("texture-null-key", [PRIM_TEXTURE, 0, NULL_KEY, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0],
            [PRIM_TEXTURE, 0]);
        rt("texture-missing-name", [PRIM_TEXTURE, 0, "no such texture", <1.0, 1.0, 0.0>,
            ZERO_VECTOR, 0.0], [PRIM_TEXTURE, 0]);
        llSetPrimitiveParams([PRIM_TEXTURE, ALL_SIDES, BLANK, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0]);

        // Bump, shiny, fullbright, glow, texgen.
        rt("bump-shiny", [PRIM_BUMP_SHINY, 0, PRIM_SHINY_HIGH, PRIM_BUMP_BRICKS], [PRIM_BUMP_SHINY, 0]);
        rt("bump-shiny-past-range", [PRIM_BUMP_SHINY, 0, 7, 40], [PRIM_BUMP_SHINY, 0]);
        llSetPrimitiveParams([PRIM_BUMP_SHINY, 0, PRIM_SHINY_NONE, PRIM_BUMP_NONE]);
        rt("fullbright", [PRIM_FULLBRIGHT, 0, TRUE], [PRIM_FULLBRIGHT, 0]);
        rt("fullbright-two", [PRIM_FULLBRIGHT, 0, 2], [PRIM_FULLBRIGHT, 0]);
        llSetPrimitiveParams([PRIM_FULLBRIGHT, 0, FALSE]);
        rt("glow", [PRIM_GLOW, 0, 0.5], [PRIM_GLOW, 0]);
        rt("glow-past-range", [PRIM_GLOW, 0, 2.0], [PRIM_GLOW, 0]);
        rt("glow-negative", [PRIM_GLOW, 0, -1.0], [PRIM_GLOW, 0]);
        rt("glow-fine", [PRIM_GLOW, 0, 0.123456], [PRIM_GLOW, 0]);
        llSetPrimitiveParams([PRIM_GLOW, 0, 0.0]);
        rt("texgen", [PRIM_TEXGEN, 0, PRIM_TEXGEN_PLANAR], [PRIM_TEXGEN, 0]);
        rt("texgen-unknown", [PRIM_TEXGEN, 0, 5], [PRIM_TEXGEN, 0]);
        llSetPrimitiveParams([PRIM_TEXGEN, 0, PRIM_TEXGEN_DEFAULT]);

        // Material, light, flexible.
        rt("material-glass", [PRIM_MATERIAL, PRIM_MATERIAL_GLASS], [PRIM_MATERIAL]);
        rt("material-light", [PRIM_MATERIAL, PRIM_MATERIAL_LIGHT], [PRIM_MATERIAL]);
        rt("material-unknown", [PRIM_MATERIAL, 8], [PRIM_MATERIAL]);
        rt("material-negative", [PRIM_MATERIAL, -1], [PRIM_MATERIAL]);
        llSetPrimitiveParams([PRIM_MATERIAL, PRIM_MATERIAL_WOOD]);
        rt("light", [PRIM_POINT_LIGHT, TRUE, <1.0, 0.5, 0.25>, 0.5, 10.0, 0.75],
            [PRIM_POINT_LIGHT]);
        rt("light-past-range", [PRIM_POINT_LIGHT, TRUE, <2.0, -1.0, 0.5>, 2.0, 30.0, 3.0],
            [PRIM_POINT_LIGHT]);
        rt("light-negative", [PRIM_POINT_LIGHT, TRUE, <1.0, 1.0, 1.0>, -1.0, -1.0, -1.0],
            [PRIM_POINT_LIGHT]);
        rt("light-off", [PRIM_POINT_LIGHT, FALSE, <1.0, 1.0, 1.0>, 0.5, 10.0, 0.75],
            [PRIM_POINT_LIGHT]);
        rt("flexible", [PRIM_FLEXIBLE, TRUE, 2, 0.3, 1.0, 0.5, 0.25, <0.5, 0.0, -0.5>],
            [PRIM_FLEXIBLE]);
        rt("flexible-past-range", [PRIM_FLEXIBLE, TRUE, 5, 20.0, -20.0, 20.0, 20.0,
            <20.0, -20.0, 20.0>], [PRIM_FLEXIBLE]);
        rt("flexible-off", [PRIM_FLEXIBLE, FALSE, 2, 0.3, 1.0, 0.5, 0.25, ZERO_VECTOR],
            [PRIM_FLEXIBLE]);

        // Text, slice, name, description.
        rt("text", [PRIM_TEXT, "hello", <1.0, 0.5, 0.0>, 0.5], [PRIM_TEXT]);
        string long = "";
        integer i;
        for (i = 0; i < 30; ++i)
        {
            long += "0123456789";
        }
        rt("text-300", [PRIM_TEXT, long, <2.0, -1.0, 0.5>, 2.0], [PRIM_TEXT]);
        emit("length", " label=text-300 got=" + (string)llStringLength(
            llList2String(llGetPrimitiveParams([PRIM_TEXT]), 0)));
        llSetPrimitiveParams([PRIM_TEXT, "", ZERO_VECTOR, 0.0]);
        rt("slice", [PRIM_SLICE, <0.25, 0.75, 0.0>], [PRIM_SLICE]);
        rt("slice-reversed", [PRIM_SLICE, <0.8, 0.2, 0.0>], [PRIM_SLICE]);
        rt("slice-past-range", [PRIM_SLICE, <-1.0, 2.0, 0.0>], [PRIM_SLICE]);
        llSetPrimitiveParams([PRIM_SLICE, <0.0, 1.0, 0.0>]);
        rt("name", [PRIM_NAME, "Probe prim"], [PRIM_NAME]);
        rt("name-100", [PRIM_NAME, llGetSubString(long, 0, 99)], [PRIM_NAME]);
        rt("desc-200", [PRIM_DESC, llGetSubString(long, 0, 199)], [PRIM_DESC]);
        rt("desc-pipe", [PRIM_DESC, "a|b"], [PRIM_DESC]);
        rt("desc-newline", [PRIM_DESC, "a\nb"], [PRIM_DESC]);
        rt("name-empty", [PRIM_NAME, ""], [PRIM_NAME]);
        llSetPrimitiveParams([PRIM_NAME, "Object", PRIM_DESC, ""]);

        // Phantom, position, rotation.
        rt("phantom", [PRIM_PHANTOM, TRUE], [PRIM_PHANTOM]);
        rt("phantom-off", [PRIM_PHANTOM, FALSE], [PRIM_PHANTOM]);
        vector home = llGetPos();
        llSetPrimitiveParams([PRIM_POSITION, home + <15.0, 0.0, 0.0>]);
        vector p = llGetPos();
        emit("position", " label=position-east-15 moved=" + (string)llRound(p.x - home.x));
        llSetRegionPos(home);
        llSetPrimitiveParams([PRIM_POS_LOCAL, home + <15.0, 0.0, 0.0>]);
        p = llGetPos();
        emit("position", " label=pos-local-east-15 moved=" + (string)llRound(p.x - home.x));
        llSetRegionPos(home);
        rt("rot-local", [PRIM_ROT_LOCAL, llEuler2Rot(<0.0, 0.0, PI_BY_TWO>)], [PRIM_ROT_LOCAL]);
        llSetPrimitiveParams([PRIM_ROT_LOCAL, ZERO_ROTATION]);

        // The material maps.
        rt("alpha-mode-mask", [PRIM_ALPHA_MODE, 0, PRIM_ALPHA_MODE_MASK, 100], [PRIM_ALPHA_MODE, 0]);
        rt("alpha-mode-cutoff-past", [PRIM_ALPHA_MODE, 0, PRIM_ALPHA_MODE_MASK, 300],
            [PRIM_ALPHA_MODE, 0]);
        rt("alpha-mode-unknown", [PRIM_ALPHA_MODE, 0, 9, 0], [PRIM_ALPHA_MODE, 0]);
        llSetPrimitiveParams([PRIM_ALPHA_MODE, 0, PRIM_ALPHA_MODE_BLEND, 0]);
        rt("normal", [PRIM_NORMAL, 0, PLYWOOD, <2.0, 2.0, 0.0>, <0.5, 0.5, 0.0>, 1.0],
            [PRIM_NORMAL, 0]);
        rt("normal-null", [PRIM_NORMAL, 0, NULL_KEY, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0],
            [PRIM_NORMAL, 0]);
        rt("specular", [PRIM_SPECULAR, 0, PLYWOOD, <2.0, 2.0, 0.0>, <0.5, 0.5, 0.0>, 1.0,
            <1.0, 0.5, 0.0>, 100, 20], [PRIM_SPECULAR, 0]);
        rt("specular-past-range", [PRIM_SPECULAR, 0, PLYWOOD, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0,
            <2.0, -1.0, 0.0>, 300, -5], [PRIM_SPECULAR, 0]);
        rt("specular-null", [PRIM_SPECULAR, 0, NULL_KEY, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0,
            <1.0, 1.0, 1.0>, 51, 0], [PRIM_SPECULAR, 0]);

        // Sitting and clicking, by number.
        rt("sit-target", [41, TRUE, <0.0, 0.0, 0.5>, llEuler2Rot(<0.0, 0.0, PI_BY_TWO>)], [41]);
        rt("sit-target-off", [41, FALSE, <0.0, 0.0, 0.5>, ZERO_ROTATION], [41]);
        rt("allow-unsit", [39, FALSE], [39]);
        llSetPrimitiveParams([39, TRUE]);
        rt("scripted-sit-only", [40, TRUE], [40]);
        llSetPrimitiveParams([40, FALSE]);
        rt("click-action-sit", [43, CLICK_ACTION_SIT], [43]);
        rt("click-action-unknown", [43, 99], [43]);
        llSetPrimitiveParams([43, CLICK_ACTION_NONE]);
        rt("sit-flags", [50, 3], [50]);
        llSetPrimitiveParams([50, 0]);

        // The newer rules, by number.
        rt("projector", [42, PLYWOOD, 1.0, 0.0, 0.5], [42]);
        llSetPrimitiveParams([42, NULL_KEY, 0.0, 0.0, 0.0]);
        rt("reflection-probe", [44, TRUE, 2.0, 0.5, 1], [44]);
        llSetPrimitiveParams([44, FALSE, 0.0, 0.0, 0]);
        rt("render-material-unknown", [49, 0, "11111111-2222-3333-4444-555555555555"], [49, 0]);
        llSetPrimitiveParams([49, 0, NULL_KEY]);
        read("gltf-base-color", [48, 0]);

        emit("end", "");
    }
}
