// probe: money/pay-price
// script: Leader
// prim: A
// order: 3
// drive: pay-price
//
// What a viewer is told of an object's pay buttons (RequestPayPrice) after
// each llSetPayPrice a script makes. The leader, which has a money event,
// asks the runner for prim A's buttons before it sets any, then after each
// of: every amount PAY_DEFAULT; an ordinary price and four buttons; every
// amount PAY_HIDE; every amount 0; negative amounts that are neither
// constant; amounts beyond what any account holds; two buttons; six
// buttons; none. Then it sets an ordinary layout again and has the other
// avatar ask, then asks prim B (whose script has no money event, see
// no-money.lsl) and prim C (buttons of other types, see mixed.lsl). Last it
// sets a layout, marks the prim's description and resets itself, and asks
// once more: whether a reset keeps the pay price. The runner notes each
// answer, the default and the buttons as sent, or that none came. A step
// lasts eight seconds, longer than the runner takes to ask.
string ID = "money/pay-price";
string MARK = "money/pay-price reset";

integer step;

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

set(string what, integer price, list buttons)
{
    llSetPayPrice(price, buttons);
    emit("set", " step=" + (string)step + " what=" + what + " price=" + (string)price
        + " buttons=" + q(llList2CSV(buttons)));
}

ask(string fields)
{
    emit("waiting", " drive=pay-price" + fields);
    llSetTimerEvent(8.0);
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        set("default", PAY_DEFAULT, [PAY_DEFAULT, PAY_DEFAULT, PAY_DEFAULT, PAY_DEFAULT]);
        ask("");
    }
    else if (step == 2)
    {
        set("ordinary", 25, [1, 5, 10, 20]);
        ask("");
    }
    else if (step == 3)
    {
        set("hide", PAY_HIDE, [PAY_HIDE, PAY_HIDE, PAY_HIDE, PAY_HIDE]);
        ask("");
    }
    else if (step == 4)
    {
        set("zero", 0, [0, 0, 0, 0]);
        ask("");
    }
    else if (step == 5)
    {
        set("negative", -5, [-7, -3, -1000, 3]);
        ask("");
    }
    else if (step == 6)
    {
        set("huge", 2000000000, [1000000, 99999, 1, 2]);
        ask("");
    }
    else if (step == 7)
    {
        set("two", 7, [1, 2]);
        ask("");
    }
    else if (step == 8)
    {
        set("six", 7, [1, 2, 3, 4, 5, 6]);
        ask("");
    }
    else if (step == 9)
    {
        set("none", 7, []);
        ask("");
    }
    else if (step == 10)
    {
        set("ordinary", 12, [2, 4, 6, 8]);
        ask(" by=other");
    }
    else if (step == 11)
    {
        emit("ask", " step=" + (string)step + " prim=B");
        ask(" prim=B");
    }
    else if (step == 12)
    {
        emit("ask", " step=" + (string)step + " prim=C");
        ask(" prim=C");
    }
    else if (step == 13)
    {
        set("before-reset", 13, [3, 6, 9, 12]);
        llSetObjectDesc(MARK);
        llResetScript();
    }
    else
    {
        llSetTimerEvent(0.0);
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        if (llGetObjectDesc() == MARK)
        {
            llSetObjectDesc("");
            step = 13;
            emit("reset", " step=" + (string)step);
            ask("");
            return;
        }
        begin();
        step = 0;
        emit("ask", " step=0 what=fresh");
        ask("");
    }

    timer()
    {
        next();
    }

    money(key id, integer amount)
    {
        emit("money", " step=" + (string)step + " amount=" + (string)amount);
    }
}
