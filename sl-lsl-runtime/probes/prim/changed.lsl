// probe: prim/changed
// drive: none
//
// Which `changed` bits each setter raises, on each grid. One step a second:
// the step's call, then every `changed` event of the second after it, OR-ed
// and counted, printed as the CHANGED_* names. The colour, alpha and texture
// setters and the texture's scale, offset and rotation; each again with the
// value it already has; scale, position and rotation; the shape, the size,
// glow, fullbright, bump and shiny, texgen, hover text, name, description, a
// point light, flexible, material, phantom, slice, llTargetOmega, a texture
// animation, particles, a sit target, sit text, the click action,
// llAllowInventoryDrop, media on a face set and cleared, the alpha mode,
// normal and specular maps, a render material by number, and two rules of
// different kinds in one call.
string ID = "prim/changed";

string PLYWOOD = "89556747-24cb-43ed-920b-47caed15465f";
string BLANK = "5748decc-f629-461c-9a36-a35a221fe21f";

list LABELS = ["color", "color-same", "alpha", "alpha-same", "texture", "texture-same",
    "scale-texture", "offset-texture", "rotate-texture", "scale", "scale-same", "position",
    "rotation", "shape", "size", "glow", "fullbright", "bump-shiny", "texgen", "text",
    "name", "desc", "light", "flexible", "material", "phantom", "slice", "target-omega",
    "texture-anim", "particles", "sit-target", "sit-text", "click-action", "allow-drop",
    "media", "media-clear", "alpha-mode", "normal", "specular", "render-material",
    "color-and-size", "link-color-this"];

integer step = -1;
integer bits;
integer events;
vector home;

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

act(string label)
{
    if (label == "color" || label == "color-same")
        llSetColor(<1.0, 0.0, 0.0>, ALL_SIDES);
    else if (label == "alpha" || label == "alpha-same")
        llSetAlpha(0.5, ALL_SIDES);
    else if (label == "texture" || label == "texture-same")
        llSetTexture(BLANK, ALL_SIDES);
    else if (label == "scale-texture")
        llScaleTexture(2.0, 2.0, ALL_SIDES);
    else if (label == "offset-texture")
        llOffsetTexture(0.25, 0.25, ALL_SIDES);
    else if (label == "rotate-texture")
        llRotateTexture(PI_BY_TWO, ALL_SIDES);
    else if (label == "scale" || label == "scale-same")
        llSetScale(<0.6, 0.6, 0.6>);
    else if (label == "position")
        llSetPos(home + <0.0, 0.0, 1.0>);
    else if (label == "rotation")
        llSetRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
    else if (label == "shape")
        llSetPrimitiveParams([PRIM_TYPE, PRIM_TYPE_BOX, PRIM_HOLE_DEFAULT, <0.0, 1.0, 0.0>, 0.5,
            ZERO_VECTOR, <1.0, 1.0, 0.0>, ZERO_VECTOR]);
    else if (label == "size")
        llSetPrimitiveParams([PRIM_SIZE, <0.5, 0.5, 0.5>]);
    else if (label == "glow")
        llSetPrimitiveParams([PRIM_GLOW, ALL_SIDES, 0.5]);
    else if (label == "fullbright")
        llSetPrimitiveParams([PRIM_FULLBRIGHT, ALL_SIDES, TRUE]);
    else if (label == "bump-shiny")
        llSetPrimitiveParams([PRIM_BUMP_SHINY, ALL_SIDES, PRIM_SHINY_HIGH, PRIM_BUMP_BRICKS]);
    else if (label == "texgen")
        llSetPrimitiveParams([PRIM_TEXGEN, ALL_SIDES, PRIM_TEXGEN_PLANAR]);
    else if (label == "text")
        llSetText("hello", <1.0, 1.0, 1.0>, 1.0);
    else if (label == "name")
        llSetObjectName("Probe prim");
    else if (label == "desc")
        llSetObjectDesc("a description");
    else if (label == "light")
        llSetPrimitiveParams([PRIM_POINT_LIGHT, TRUE, <1.0, 1.0, 1.0>, 1.0, 10.0, 0.75]);
    else if (label == "flexible")
        llSetPrimitiveParams([PRIM_FLEXIBLE, TRUE, 2, 0.3, 1.0, 0.5, 0.25, ZERO_VECTOR]);
    else if (label == "material")
        llSetPrimitiveParams([PRIM_MATERIAL, PRIM_MATERIAL_GLASS]);
    else if (label == "phantom")
        llSetStatus(STATUS_PHANTOM, TRUE);
    else if (label == "slice")
        llSetPrimitiveParams([PRIM_SLICE, <0.25, 0.75, 0.0>]);
    else if (label == "target-omega")
        llTargetOmega(<0.0, 0.0, 1.0>, 1.0, 1.0);
    else if (label == "texture-anim")
        llSetTextureAnim(ANIM_ON | LOOP, ALL_SIDES, 4, 4, 0.0, 16.0, 8.0);
    else if (label == "particles")
        llParticleSystem([PSYS_PART_MAX_AGE, 1.0, PSYS_SRC_BURST_RATE, 0.5]);
    else if (label == "sit-target")
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
    else if (label == "sit-text")
        llSetSitText("Perch");
    else if (label == "click-action")
        llSetClickAction(CLICK_ACTION_SIT);
    else if (label == "allow-drop")
        llAllowInventoryDrop(TRUE);
    else if (label == "media")
        llSetPrimMediaParams(0, [PRIM_MEDIA_HOME_URL, "http://example.com/"]);
    else if (label == "media-clear")
        llClearPrimMedia(0);
    else if (label == "alpha-mode")
        llSetPrimitiveParams([PRIM_ALPHA_MODE, ALL_SIDES, PRIM_ALPHA_MODE_MASK, 128]);
    else if (label == "normal")
        llSetPrimitiveParams([PRIM_NORMAL, ALL_SIDES, PLYWOOD, <1.0, 1.0, 0.0>, ZERO_VECTOR, 0.0]);
    else if (label == "specular")
        llSetPrimitiveParams([PRIM_SPECULAR, ALL_SIDES, PLYWOOD, <1.0, 1.0, 0.0>, ZERO_VECTOR,
            0.0, <1.0, 1.0, 1.0>, 51, 0]);
    else if (label == "render-material")
        llSetPrimitiveParams([49, ALL_SIDES, "11111111-2222-3333-4444-555555555555"]);
    else if (label == "color-and-size")
        llSetPrimitiveParams([PRIM_COLOR, ALL_SIDES, <0.0, 1.0, 0.0>, 1.0,
            PRIM_SIZE, <0.7, 0.7, 0.7>]);
    else if (label == "link-color-this")
        llSetLinkColor(LINK_THIS, <0.0, 0.0, 1.0>, ALL_SIDES);
}

next()
{
    ++step;
    if (step >= llGetListLength(LABELS))
    {
        llSetTimerEvent(0.0);
        llTargetOmega(ZERO_VECTOR, 0.0, 0.0);
        llParticleSystem([]);
        llSetTextureAnim(FALSE, ALL_SIDES, 0, 0, 0.0, 0.0, 0.0);
        llSetStatus(STATUS_PHANTOM, FALSE);
        llSetRegionPos(home);
        emit("end", "");
        return;
    }
    bits = 0;
    events = 0;
    string label = llList2String(LABELS, step);
    emit("try", " label=" + label);
    act(label);
    llSetTimerEvent(1.0);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        llSetTimerEvent(1.0);
    }

    changed(integer change)
    {
        bits = bits | change;
        ++events;
        if (step < 0)
        {
            emit("early", " bits=" + names(change));
        }
    }

    timer()
    {
        if (step >= llGetListLength(LABELS))
        {
            return;
        }
        if (step >= 0)
        {
            emit("changed", " label=" + llList2String(LABELS, step) + " bits=" + names(bits)
                + " events=" + (string)events);
        }
        next();
    }
}
