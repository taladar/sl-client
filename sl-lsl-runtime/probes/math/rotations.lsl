// probe: math/rotations
// drive: none
//
// The rotation helpers, for their axis order and what they do with input
// that is not a unit rotation or a unit vector: llEuler2Rot and llRot2Euler
// one axis at a time, together and at the gimbal, the order a product
// applies its factors in, a vector rotated by a rotation that is not
// normalised, llAxisAngle2Rot of an axis that is not a unit vector or is
// zero, llRot2Axis and llRot2Angle at the identity, its negation and a
// negative s, llRotBetween of parallel, opposite and zero vectors,
// llAngleBetween of a rotation and its negation, llAxes2Rot of axes that are
// scaled, skewed or left-handed, llRot2Fwd / Left / Up of a scaled
// rotation, and rotation addition and subtraction (negating a rotation is
// math/negate-rotation's, since XEngine does not compile it). Every value
// comes out of a list built at run time, so no compiler folds it.
string ID = "math/rotations";

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

// A vector or rotation with six decimals per component, where (string)
// gives five.
string v6(vector v)
{
    return "<" + (string)v.x + "," + (string)v.y + "," + (string)v.z + ">";
}

string r6(rotation r)
{
    return "<" + (string)r.x + "," + (string)r.y + "," + (string)r.z + "," + (string)r.s + ">";
}

out(string rec, string name, string value)
{
    emit(rec, " case=" + name + " value=" + value);
}

list V;

vector vec(integer i)
{
    return llList2Vector(V, i);
}

list R;

rotation rot(integer i)
{
    return llList2Rot(R, i);
}

default
{
    state_entry()
    {
        begin();
        float d = DEG_TO_RAD;
        V = [
            <90.0, 0.0, 0.0>, <0.0, 90.0, 0.0>, <0.0, 0.0, 90.0>,
            <30.0, 45.0, 60.0>, <10.0, 90.0, 20.0>, <10.0, -90.0, 20.0>,
            <400.0, -200.0, 1000.0>, <1.0, 0.0, 0.0>, <0.0, 1.0, 0.0>,
            <0.0, 0.0, 1.0>, <0.0, 0.0, 2.0>, <0.0, 0.0, 0.0>,
            <-1.0, 0.0, 0.0>, <3.0, 4.0, 0.0>, <1.0, 2.0, 3.0>,
            <0.0, 0.0, 0.0000001>, <2.0, 0.0, 0.0>, <1.0, 1.0, 0.0>
        ];
        R = [
            <0.0, 0.0, 0.0, 1.0>, <0.0, 0.0, 0.0, -1.0>, <0.0, 0.0, 0.0, 2.0>,
            <1.0, 2.0, 3.0, 4.0>, <0.0, 0.0, 0.0, 0.0>,
            <0.0, 0.0, 0.7071068, -0.7071068>, <0.5, 0.5, 0.5, 0.5>
        ];
        integer i;
        // Euler angles, one axis, three axes, the gimbal and past a turn.
        list names = ["x90", "y90", "z90", "xyz30-45-60", "gimbal-up", "gimbal-down", "past-a-turn"];
        for (i = 0; i < 7; ++i)
        {
            rotation e = llEuler2Rot(vec(i) * d);
            out("euler2rot", llList2String(names, i), r6(e));
            out("euler2rot-back", llList2String(names, i), v6(llRot2Euler(e) / d));
        }
        // The order a product applies its factors in: x90 * z90 against
        // z90 * x90, and a vector through each.
        rotation x90 = llEuler2Rot(vec(0) * d);
        rotation z90 = llEuler2Rot(vec(2) * d);
        rotation xz = x90 * z90;
        rotation zx = z90 * x90;
        out("compose", "x90*z90", r6(xz));
        out("compose", "z90*x90", r6(zx));
        out("compose", "y-axis-through-x90*z90", v6(vec(8) * xz));
        out("compose", "y-axis-through-x90-then-z90", v6((vec(8) * x90) * z90));
        out("compose", "y-axis-through-z90*x90", v6(vec(8) * zx));
        // Rotating by rotations that are not unit length.
        out("rotate", "x-by-twice-identity", v6(vec(7) * rot(2)));
        out("rotate", "x-by-1234", v6(vec(7) * rot(3)));
        out("rotate", "x-by-zero", v6(vec(7) * rot(4)));
        out("unrotate", "x-by-1234", v6(vec(7) / rot(3)));
        out("rotate", "x-by-negative-identity", v6(vec(7) * rot(1)));
        out("compose", "1234*1234", r6(rot(3) * rot(3)));
        out("divide", "1234/1234", r6(rot(3) / rot(3)));
        out("divide", "x90/x90", r6(x90 / x90));
        // Axis and angle.
        out("axis-angle", "z-90", r6(llAxisAngle2Rot(vec(9), 90.0 * d)));
        out("axis-angle", "z-length-2-90", r6(llAxisAngle2Rot(vec(10), 90.0 * d)));
        out("axis-angle", "zero-axis-90", r6(llAxisAngle2Rot(vec(11), 90.0 * d)));
        out("axis-angle", "tiny-axis-90", r6(llAxisAngle2Rot(vec(15), 90.0 * d)));
        out("axis-angle", "z-450", r6(llAxisAngle2Rot(vec(9), 450.0 * d)));
        out("axis-angle", "z-minus-90", r6(llAxisAngle2Rot(vec(9), -90.0 * d)));
        list axes = ["identity", "negative-identity", "twice-identity", "1234", "zero", "z-negative-s", "half"];
        for (i = 0; i < 7; ++i)
        {
            string n = llList2String(axes, i);
            out("rot2axis", n, v6(llRot2Axis(rot(i))));
            out("rot2angle", n, (string)llRot2Angle(rot(i)));
            out("rot2euler", n, v6(llRot2Euler(rot(i))));
            out("rot2fwd", n, v6(llRot2Fwd(rot(i))));
            out("rot2left", n, v6(llRot2Left(rot(i))));
            out("rot2up", n, v6(llRot2Up(rot(i))));
        }
        // Between two vectors.
        out("rot-between", "x-y", r6(llRotBetween(vec(7), vec(8))));
        out("rot-between", "x-x", r6(llRotBetween(vec(7), vec(7))));
        out("rot-between", "x-minus-x", r6(llRotBetween(vec(7), vec(12))));
        out("rot-between", "z-minus-x", r6(llRotBetween(vec(9), vec(12))));
        out("rot-between", "x-zero", r6(llRotBetween(vec(7), vec(11))));
        out("rot-between", "zero-zero", r6(llRotBetween(vec(11), vec(11))));
        out("rot-between", "x-twice-x", r6(llRotBetween(vec(7), vec(16))));
        out("rot-between", "x-34", r6(llRotBetween(vec(7), vec(13))));
        out("rot-between", "123-x", r6(llRotBetween(vec(14), vec(7))));
        out("angle-between", "identity-negative-identity", (string)llAngleBetween(rot(0), rot(1)));
        out("angle-between", "identity-twice-identity", (string)llAngleBetween(rot(0), rot(2)));
        out("angle-between", "identity-1234", (string)llAngleBetween(rot(0), rot(3)));
        out("angle-between", "x90-z90", (string)llAngleBetween(x90, z90));
        out("angle-between", "identity-zero", (string)llAngleBetween(rot(0), rot(4)));
        // Axes to a rotation.
        out("axes2rot", "unit", r6(llAxes2Rot(vec(7), vec(8), vec(9))));
        out("axes2rot", "x-twice", r6(llAxes2Rot(vec(16), vec(8), vec(9))));
        out("axes2rot", "left-handed", r6(llAxes2Rot(vec(7), vec(8), vec(9) * -1.0)));
        out("axes2rot", "skewed", r6(llAxes2Rot(vec(7), vec(17), vec(9))));
        out("axes2rot", "z90-axes", r6(llAxes2Rot(vec(8), vec(12), vec(9))));
        // Addition, subtraction and negating a vector.
        rotation a = rot(3);
        rotation b = rot(6);
        out("add", "1234+half", r6(a + b));
        out("subtract", "1234-half", r6(a - b));
        out("negate", "x-axis", v6(-vec(7)));
        // Vector products and scaling.
        out("dot", "123.x", (string)(vec(14) * vec(7)));
        out("cross", "x%y", v6(vec(7) % vec(8)));
        out("scale", "123*2", v6(vec(14) * 2.0));
        out("scale", "123/2", v6(vec(14) / 2.0));
        out("normalise", "tiny", v6(llVecNorm(vec(15))));
        out("magnitude", "34", (string)llVecMag(vec(13)));
        out("distance", "x-y", (string)llVecDist(vec(7), vec(8)));
        emit("end", "");
    }
}
