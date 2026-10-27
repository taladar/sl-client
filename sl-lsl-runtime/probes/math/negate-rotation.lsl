// probe: math/negate-rotation
// drive: none
//
// Unary minus on a rotation and on a vector, in a probe of its own because
// XEngine does not compile it for a rotation (CS0023), which would cost
// math/rotations every other line.
string ID = "math/negate-rotation";

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

default
{
    state_entry()
    {
        begin();
        list l = [<1.0, 2.0, 3.0, 4.0>, <1.0, 0.0, 0.0>];
        rotation r = llList2Rot(l, 0);
        vector v = llList2Vector(l, 1);
        emit("negate", " case=rotation value=" + q((string)(-r)));
        emit("negate", " case=vector value=" + q((string)(-v)));
        emit("end", "");
    }
}
