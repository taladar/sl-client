// probe: detection/touch-surface
// drive: touch
//
// What a script reads of where a touch landed: llDetectedTouchFace, UV, ST,
// Pos, Normal and Binormal in touch_start and touch_end, against what the
// viewer sent in the grab's surface block (the runner notes what it sent;
// Pos as an offset from prim A's centre). Five touches: one with no
// surface block, as an old viewer sends it; one a viewer could have picked
// (face 1, a point on the +X face); one no viewer could pick (face 9 of a
// six-faced cube, coordinates outside 0..1, a point 8.7 m away, a normal
// of length 2, a zero binormal); one that says face -1 yet carries the
// rest; and one from the other avatar. Each prints after its touch_end.
string ID = "detection/touch-surface";

integer step;
integer touches;

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

string num(float f)
{
    return (string)(llRound(f * 1000.0) / 1000.0);
}

string vec(vector v)
{
    return "<" + num(v.x) + "," + num(v.y) + "," + num(v.z) + ">";
}

string read(string event_name)
{
    vector pos = llDetectedTouchPos(0);
    string offset = "zero";
    if (pos != ZERO_VECTOR)
    {
        offset = vec(pos - llGetPos());
    }
    return " step=" + (string)step + " event=" + event_name
        + " key_is_owner=" + (string)(llDetectedKey(0) == llGetOwner())
        + " face=" + (string)llDetectedTouchFace(0)
        + " uv=" + vec(llDetectedTouchUV(0))
        + " st=" + vec(llDetectedTouchST(0))
        + " pos=" + offset
        + " normal=" + vec(llDetectedTouchNormal(0))
        + " binormal=" + vec(llDetectedTouchBinormal(0))
        + " grab=" + vec(llDetectedGrab(0))
        + " link=" + (string)llDetectedLinkNumber(0);
}

ask()
{
    step = step + 1;
    if (step == 1)
    {
        emit("waiting", " drive=touch");
    }
    else if (step == 2)
    {
        emit("waiting", " drive=touch face=1 uv=0.25,0.75 st=0.3,0.6 at=0.25,0.05,0.1"
            + " normal=1,0,0 binormal=0,0,1");
    }
    else if (step == 3)
    {
        emit("waiting", " drive=touch face=9 uv=1.5,-0.5 st=2,-1 at=5,5,5"
            + " normal=0,0,2 binormal=0,0,0");
    }
    else if (step == 4)
    {
        emit("waiting", " drive=touch face=-1 uv=0.5,0.5 st=0.5,0.5 at=0,0,0.25"
            + " normal=0,0,1 binormal=0,1,0");
    }
    else if (step == 5)
    {
        emit("waiting", " drive=touch by=other face=2 uv=0.1,0.9 st=0.2,0.8 at=-0.25,0,0"
            + " normal=-1,0,0 binormal=0,0,1");
    }
    else
    {
        emit("touches", " held=" + (string)touches);
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        begin();
        emit("constants", " invalid_face=" + (string)TOUCH_INVALID_FACE
            + " invalid_texcoord=" + vec(TOUCH_INVALID_TEXCOORD)
            + " invalid_vector=" + vec(TOUCH_INVALID_VECTOR));
        ask();
    }

    touch_start(integer count)
    {
        emit("touch_start", read("touch_start"));
    }

    touch(integer count)
    {
        touches = touches + 1;
    }

    touch_end(integer count)
    {
        emit("touch_end", read("touch_end"));
        ask();
    }
}
