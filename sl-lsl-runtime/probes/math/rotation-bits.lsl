// probe: math/rotation-bits
// drive: none
//
// The exact bits of rotation, vector and float arithmetic, so the precision a
// grid computes in can be read off: `rotation * rotation`, `rotation /
// rotation`, `vector * rotation`, `vector / rotation`, the dot and cross
// products, a sum of two products in one expression and in two statements,
// and a few library functions. (string) gives six decimals, which cannot
// tell a float from its neighbour, so every value is printed as its mantissa
// and exponent, found by scaling by two: `<sign><hi>:<lo>p<exp>` is
// (hi + lo / 2^29) * 2^exp with hi in [2^23, 2^24) — lo is 0 for a value
// that fits in a float, and holds the rest of a double's 53 bits on OpenSim.
// The first pair is PyOptimizer's open case, `<3, 5, 7, 17> * <.22, .26, .38,
// .86>`, whose y is said to be exactly 8.32 on Second Life; the others were
// chosen so that summing in floats, summing in doubles and rounding each
// product first all give different answers.
string ID = "math/rotation-bits";

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

// The value's sign, mantissa and exponent.
string bits(float f)
{
    if (f != f) return "nan";
    if (f == 0.0) return "0";
    string sign = "+";
    float a = f;
    if (a < 0.0)
    {
        sign = "-";
        a = -a;
    }
    if (a == a * 2.0) return sign + "inf";
    integer e = 0;
    while (a >= 16777216.0)
    {
        a = a / 2.0;
        ++e;
    }
    while (a < 8388608.0)
    {
        a = a * 2.0;
        --e;
    }
    integer hi = (integer)a;
    float rest = a - (float)hi;
    integer lo = (integer)(rest * 536870912.0);
    return sign + (string)hi + ":" + (string)lo + "p" + (string)e;
}

string vbits(vector v)
{
    return " x=" + bits(v.x) + " y=" + bits(v.y) + " z=" + bits(v.z);
}

string rbits(rotation r)
{
    return " x=" + bits(r.x) + " y=" + bits(r.y) + " z=" + bits(r.z) + " s=" + bits(r.s);
}

pair(integer n, rotation a, rotation b)
{
    string c = " case=" + (string)n;
    vector v = <a.x, a.y, a.z>;
    vector w = <b.x, b.y, b.z>;
    emit("input", c + " a=" + q((string)a) + " b=" + q((string)b));
    emit("input-bits", c + " operand=a" + rbits(a));
    emit("input-bits", c + " operand=b" + rbits(b));
    rotation r = a * b;
    emit("compose", c + rbits(r));
    r = a / b;
    emit("divide", c + rbits(r));
    vector u = v * b;
    emit("rotate", c + vbits(u));
    u = v / b;
    emit("unrotate", c + vbits(u));
    float d = v * w;
    emit("dot", c + " value=" + bits(d));
    u = v % w;
    emit("cross", c + vbits(u));
    float s = a.x * b.x + a.y * b.y;
    emit("two-products", c + " form=one-expression value=" + bits(s));
    float p = a.x * b.x;
    float t = a.y * b.y;
    s = p + t;
    emit("two-products", c + " form=two-statements value=" + bits(s));
    s = a.x * b.x * a.y;
    emit("three-factors", c + " form=one-expression value=" + bits(s));
    s = p * a.y;
    emit("three-factors", c + " form=two-statements value=" + bits(s));
    emit("magnitude", c + " value=" + bits(llVecMag(v)));
    emit("normalise", c + vbits(llVecNorm(v)));
    emit("rot-between", c + rbits(llRotBetween(v, w)));
    emit("angle-between", c + " value=" + bits(llAngleBetween(a, b)));
    emit("rot2euler", c + vbits(llRot2Euler(a)));
    emit("euler2rot", c + rbits(llEuler2Rot(v)));
    emit("axis-angle", c + rbits(llAxisAngle2Rot(v, b.s)));
    emit("rot2axis", c + vbits(llRot2Axis(a)) + " angle=" + bits(llRot2Angle(a)));
    emit("rot2fwd", c + vbits(llRot2Fwd(a)));
}

default
{
    state_entry()
    {
        begin();
        list rots = [
            <3.0, 5.0, 7.0, 17.0>, <0.22, 0.26, 0.38, 0.86>,
            <-0.925, -0.133, -0.86, -0.819>, <-0.151, 0.654, -0.752, -0.554>,
            <-0.094, -0.4, 0.589, 0.398>, <-0.512, 0.149, 0.05, 0.75>,
            <-0.922, 0.336, 0.529, 0.146>, <0.751, -0.373, 0.391, 0.189>,
            <0.16, -0.088, 0.68, 0.889>, <-0.052, 0.328, -0.879, 0.403>,
            <-0.664, -0.766, -0.882, 0.536>, <-0.741, -0.505, -0.218, 0.743>,
            <-0.282, 0.768, 0.915, -0.698>, <-0.648, -0.536, -0.533, -0.03>
        ];
        integer i;
        for (i = 0; i < llGetListLength(rots); i += 2)
        {
            pair(i / 2, llList2Rot(rots, i), llList2Rot(rots, i + 1));
        }
        list xs = [2.0, 0.1, 1.0, 3.0, 1.1, 10.0, 1000000.0];
        float x = llList2Float(xs, 0);
        emit("library", " fn=llSqrt arg=2 value=" + bits(llSqrt(x)));
        x = llList2Float(xs, 1);
        emit("library", " fn=literal arg=0.1 value=" + bits(x));
        x = llList2Float(xs, 2);
        emit("library", " fn=llSin arg=1 value=" + bits(llSin(x)));
        emit("library", " fn=llCos arg=1 value=" + bits(llCos(x)));
        emit("library", " fn=llTan arg=1 value=" + bits(llTan(x)));
        emit("library", " fn=llAtan2 arg=1,3 value=" + bits(llAtan2(x, llList2Float(xs, 3))));
        emit("library", " fn=llPow arg=1.1,2.5 value=" + bits(llPow(llList2Float(xs, 4), 2.5)));
        emit("library", " fn=llLog arg=10 value=" + bits(llLog(llList2Float(xs, 5))));
        emit("library", " fn=llLog10 arg=1000000 value=" + bits(llLog10(llList2Float(xs, 6))));
        emit("library", " fn=llSin arg=1000000 value=" + bits(llSin(llList2Float(xs, 6))));
        emit("library", " fn=llAsin arg=0.1 value=" + bits(llAsin(llList2Float(xs, 1))));
        emit("library", " fn=divide arg=1,3 value=" + bits(x / llList2Float(xs, 3)));
        emit("library", " fn=PI value=" + bits(PI));
        emit("library", " fn=DEG_TO_RAD value=" + bits(DEG_TO_RAD));
        emit("library", " fn=SQRT2 value=" + bits(SQRT2));
        emit("end", "");
    }
}
