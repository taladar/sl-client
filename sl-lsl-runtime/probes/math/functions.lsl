// probe: math/functions
// drive: none
//
// The maths functions and integer operators at their edges, where a grid
// may answer differently: llRound at halves, llFloor / llCeil / llRound /
// (integer) of a float past the integer range, llAbs of the least integer,
// overflow of + and *, shifts by 32 and more and by a negative count,
// llPow, llLog and the trigonometry at their corners, llAtan2 of zeros,
// llModPow with negative and large operands, and the range of llFrand for
// a positive, a negative and a zero magnitude over 2000 draws. The run-time
// errors tranche (errors/math) has the cases that fault; none here does on
// any grid. Every operand comes out of a list built at run time.
string ID = "math/functions";

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

out(string rec, string name, string value)
{
    emit(rec, " case=" + name + " value=" + q(value));
}

list F;

float f(integer i)
{
    return llList2Float(F, i);
}

list I;

integer n(integer i)
{
    return llList2Integer(I, i);
}

frand(string name, float mag)
{
    float lo = 1.0e30;
    float hi = -1.0e30;
    integer zeros;
    integer i;
    for (i = 0; i < 2000; ++i)
    {
        float r = llFrand(mag);
        if (r < lo) lo = r;
        if (r > hi) hi = r;
        if (r == 0.0) ++zeros;
    }
    emit("frand", " case=" + name + " draws=2000 below_zero=" + (string)(lo < 0.0)
        + " above_zero=" + (string)(hi > 0.0) + " zeros=" + (string)zeros
        + " min=" + (string)lo + " max=" + (string)hi);
}

default
{
    state_entry()
    {
        begin();
        F = [0.5, 1.5, 2.5, -0.5, -1.5, -2.5, 2147483648.0, -2147483904.0,
            1.0e10, -1.0e10, 0.49999997, -0.0, 0.0, 1.0, 2.0, -8.0, 1.0e6, 1.0e-6,
            1.0e30, 3.0, 2.718281828];
        I = [-2147483648, 2147483647, 1, -1, 32, 33, -1, 31, 0, 3, 5, 7, -8, 1000000007];
        integer i;
        for (i = 0; i < 6; ++i)
        {
            string a = (string)f(i);
            out("round", a, (string)llRound(f(i)));
            out("floor", a, (string)llFloor(f(i)));
            out("ceil", a, (string)llCeil(f(i)));
            out("cast-integer", a, (string)((integer)f(i)));
        }
        out("round", "0.49999997", (string)llRound(f(10)));
        list big = ["2147483648", "-2147483904", "1e10", "-1e10"];
        for (i = 0; i < 4; ++i)
        {
            string a = llList2String(big, i);
            out("round", a, (string)llRound(f(i + 6)));
            out("floor", a, (string)llFloor(f(i + 6)));
            out("ceil", a, (string)llCeil(f(i + 6)));
            out("cast-integer", a, (string)((integer)f(i + 6)));
        }
        out("abs", "int-min", (string)llAbs(n(0)));
        out("negate", "int-min", (string)(-n(0)));
        out("fabs", "minus-zero", (string)llFabs(f(11)));
        out("add", "int-max+1", (string)(n(1) + n(2)));
        out("subtract", "int-min-1", (string)(n(0) - n(2)));
        out("multiply", "int-max*2", (string)(n(1) * 2));
        out("multiply", "1000000007*1000000007", (string)(n(13) * n(13)));
        out("shift-left", "1<<31", (string)(n(2) << n(7)));
        out("shift-left", "1<<32", (string)(n(2) << n(4)));
        out("shift-left", "1<<33", (string)(n(2) << n(5)));
        out("shift-left", "1<<-1", (string)(n(2) << n(6)));
        out("shift-right", "-8>>1", (string)(n(12) >> n(2)));
        out("shift-right", "-1>>32", (string)(n(3) >> n(4)));
        out("shift-right", "int-min>>31", (string)(n(0) >> n(7)));
        out("shift-right", "int-min>>-1", (string)(n(0) >> n(6)));
        out("not", "int-min", (string)(~n(0)));
        out("mod", "-7%3", (string)(-n(11) % n(9)));
        out("mod", "7%-3", (string)(n(11) % -n(9)));
        out("divide", "-7/2", (string)(-n(11) / 2));
        out("float-mod-like", "int-of-float-7.9", (string)((integer)(f(14) * 3.95)));
        out("pow", "0^0", (string)llPow(f(12), f(12)));
        out("pow", "-8^(1/3)", (string)llPow(f(15), f(13) / f(19)));
        out("pow", "-8^2", (string)llPow(f(15), f(14)));
        out("pow", "2^0.5", (string)llPow(f(14), f(0)));
        out("pow", "1e30^2", (string)llPow(f(18), f(14)));
        out("pow", "1e-6^-7", (string)llPow(f(17), -7.0));
        out("sqrt", "0", (string)llSqrt(f(12)));
        out("sqrt", "minus-zero", (string)llSqrt(f(11)));
        out("log", "e", (string)llLog(f(20)));
        out("log", "1", (string)llLog(f(13)));
        out("log10", "1e6", (string)llLog10(f(16)));
        out("log10", "1e-6", (string)llLog10(f(17)));
        out("sin", "1e6", (string)llSin(f(16)));
        out("cos", "1e6", (string)llCos(f(16)));
        out("tan", "pi/2", (string)llTan(PI_BY_TWO));
        out("tan", "1e30", (string)llTan(f(18)));
        out("atan2", "0,0", (string)llAtan2(f(12), f(12)));
        out("atan2", "0,-1", (string)llAtan2(f(12), -f(13)));
        out("atan2", "-0,-1", (string)llAtan2(f(11), -f(13)));
        out("atan2", "1,0", (string)llAtan2(f(13), f(12)));
        out("asin", "1", (string)llAsin(f(13)));
        out("acos", "-1", (string)llAcos(-f(13)));
        out("modpow", "2,10,1000", (string)llModPow(2, n(5) - 23, 1000));
        out("modpow", "-2,3,7", (string)llModPow(-2, n(9), 7));
        out("modpow", "2,-1,7", (string)llModPow(2, n(3), 7));
        out("modpow", "2,3,-7", (string)llModPow(2, n(9), -n(11)));
        out("modpow", "7,1000000007,1000000007", (string)llModPow(n(11), n(13), n(13)));
        out("modpow", "int-max,int-max,int-max-1", (string)llModPow(n(1), n(1), n(1) - 1));
        out("modpow", "3,int-max,65537", (string)llModPow(3, n(1), 65537));
        frand("one", f(13));
        frand("minus-one", -f(13));
        frand("zero", f(12));
        frand("1e30", f(18));
        emit("end", "");
    }
}
