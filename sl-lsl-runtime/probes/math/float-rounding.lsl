// probe: math/float-rounding
// drive: none
//
// Where an expression of floats is rounded to a float. `math/rotation-bits`
// showed Second Life's Mono evaluating `a * b + c * d` in double and rounding
// only when the sum is stored; this asks every other place a value can go.
//
// The detector is a residual: `a * b` of the two floats 1.1 and 1.3 is exact
// in double and not a float, and `R` holds it stored, so rounded. A value
// that reaches a point unrounded and is then taken `- R` in double leaves
// the residual (about 3.1e-8, which is itself a float, so storing it keeps
// it); one the point rounded leaves 0. Each `rec=point` names the point and
// prints its residual with `bits` (mantissa and exponent, as in
// rotation-bits), or the one value that tells the cases apart.
//
// Every operand comes out of a list built at run time, so no compiler folds
// it.
string ID = "math/float-rounding";

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

// The value's sign, mantissa and exponent: `<sign><hi>:<lo>p<exp>` is
// (hi + lo / 2^29) * 2^exp with hi in [2^23, 2^24).
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

point(string name, float residual)
{
    emit("point", " name=" + name + " value=" + bits(residual));
}

flag(string name, integer value)
{
    emit("point", " name=" + name + " value=" + (string)value);
}

float A;
float B;
float R;

// The argument as it arrives, less R.
float minus_r(float x)
{
    float d = x - R;
    return d;
}

// The product, returned as an expression.
float product(float x, float y)
{
    return x * y;
}

default
{
    state_entry()
    {
        begin();
        list xs = [1.1, 1.3, 0.4, 0.1, 16777216.0, 16777217, 1.4999996,
            2.0000005, 1.05, 3.86, 1.0e-30, 1.0e-20, 1.0e30, 1.0,
            5.9604645e-08, 8.6736174e-19, 2.0];
        A = llList2Float(xs, 0);
        B = llList2Float(xs, 1);
        R = A * B;
        emit("input", " a=" + bits(A) + " b=" + bits(B) + " r=" + bits(R));

        // The detector: unrounded in one expression, rounded once stored.
        float s = A * B - R;
        point("expression", s);
        float t = A * B;
        s = t - R;
        point("local", s);
        // A quotient taken back: exactly A in double, a residual of the
        // rounded quotient otherwise.
        s = A / B * B - A;
        point("quotient", s);

        // A call's argument, and what it returns.
        point("user-argument", minus_r(A * B));
        s = product(A, B) - R;
        point("user-return", s);
        s = llFabs(A * B) - R;
        point("library-argument", s);
        float two = llList2Float(xs, 16);
        float root = llSqrt(two);
        s = llSqrt(two) - root;
        point("library-return", s);
        s = -(A * B) + R;
        point("negation", s);

        // Into a list, a vector and a rotation, and back out.
        s = llList2Float([A * B], 0) - R;
        point("list-element", s);
        list l = [];
        l += A * B;
        s = llList2Float(l, 0) - R;
        point("list-append", s);
        vector v = <A * B, 0.0, 0.0>;
        s = v.x - R;
        point("vector-component", s);
        rotation r = <A * B, 0.0, 0.0, 1.0>;
        s = r.x - R;
        point("rotation-component", s);
        v.y = A * B;
        s = v.y - R;
        point("member-store", s);

        // Casts.
        s = (float)(A * B) - R;
        point("cast-float", s);
        integer i = (integer)(llList2Float(xs, 6) * llList2Float(xs, 7));
        flag("cast-integer", i);
        emit("point", " name=cast-string value="
            + (string)(llList2Float(xs, 8) * llList2Float(xs, 9)));

        // A comparison and a condition.
        flag("equal", A * B == R);
        flag("not-equal", A * B != R);
        flag("greater", A * B > R);
        float tiny = llList2Float(xs, 10);
        float tinier = llList2Float(xs, 11);
        integer taken = 0;
        if (tiny * tinier) taken = 1;
        flag("condition", taken);

        // The value of an assignment, `+=`, `++` and `--`.
        s = (t = A * B) - R;
        point("assignment-value", s);
        float c = llList2Float(xs, 2);
        s = c;
        s += A * B;
        point("compound", s);
        s = c + A * B;
        point("compound-one-expression", s);
        s = c + R;
        point("compound-two-statements", s);
        s = c;
        t = (s += A * B) - (c + R);
        point("compound-value", t);
        float big = llList2Float(xs, 4);
        s = big;
        t = (++s) - big;
        point("increment-value", t);
        t = s - big;
        point("increment-stored", t);
        s = big;
        t = (s += 1.0) - big;
        point("add-assign-value", t);

        // An integer meeting a float.
        integer n = llList2Integer(xs, 5);
        s = (n * 1.0) - big;
        point("integer-operand", s);
        s = (float)n - big;
        point("integer-cast", s);
        s = n;
        s = s - big;
        point("integer-stored", s);

        // The vector and rotation operators.
        float k = llList2Float(xs, 3);
        v = <k, 0.0, 0.0> * (A * B);
        emit("point", " name=vector-scale value=" + bits(v.x)
            + " rounded=" + bits(k * R));
        v = <A, B, 0.0>;
        vector w = <B, A, 0.0>;
        float d = v * w;
        s = v * w - d;
        point("dot-in-expression", s);

        // Out of range and back, and one past double's precision.
        float huge = llList2Float(xs, 12);
        s = huge * huge / huge;
        point("overflow-and-back", s);
        float one = llList2Float(xs, 13);
        float half = llList2Float(xs, 14);
        float least = llList2Float(xs, 15);
        s = one + half + least;
        point("extended", s);
        emit("end", "");
    }
}
