// probe: money/pay
// script: Till
// order: 2
// drive: pay, answer
//
// What a script's money event is told of a payment, and which payments a
// grid takes. Prim A holds this leader and a second script with a money
// event (second-till.lsl). The runner pays prim A, as a viewer's pay dialog
// does: the owner L$ 1; the other avatar L$ 1; L$ 0; then, under each of
// four pay layouts, an amount the layout offers or not — L$ 7 when the
// price is 5 and the buttons 1 to 4; L$ 1 when the price and every button
// are hidden; L$ 7 and L$ 2 when the price is hidden and the buttons are 1
// to 4; L$ 7 and L$ 5 when the price is 5 and every button is hidden. Then
// the layout is put back to the default, the leader goes to a state with no
// money event and the other avatar pays L$ 1, which only the second script
// can hear; then the second script removes itself and the owner pays L$ 1
// to an object none of whose scripts has a money event now. Back in its first state, the leader asks the owner
// for PERMISSION_DEBIT and gives the other avatar back what it heard it pay,
// so a run costs that avatar nothing. The runner notes each payment and the
// balance each avatar is told afterwards, as a change. A step lasts ten
// seconds.
string ID = "money/pay";

integer step;
integer phase;
integer paid;
key other;

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

string who(key id)
{
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (id == NULL_KEY)
    {
        return "none";
    }
    return "other";
}

heard(key id, integer amount)
{
    if (id != llGetOwner() && id != NULL_KEY)
    {
        other = id;
        paid = paid + amount;
    }
}

set(integer price, list buttons)
{
    llSetPayPrice(price, buttons);
    emit("set", " step=" + (string)step + " price=" + (string)price
        + " buttons=" + q(llList2CSV(buttons)));
}

pay(string by, integer amount)
{
    emit("waiting", " drive=pay" + by + " amount=" + (string)amount);
    llSetTimerEvent(10.0);
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        pay("", 1);
    }
    else if (step == 2)
    {
        pay(" by=other", 1);
    }
    else if (step == 3)
    {
        pay(" by=other", 0);
    }
    else if (step == 4)
    {
        set(5, [1, 2, 3, 4]);
        pay(" by=other", 7);
    }
    else if (step == 5)
    {
        set(PAY_HIDE, [PAY_HIDE, PAY_HIDE, PAY_HIDE, PAY_HIDE]);
        pay(" by=other", 1);
    }
    else if (step == 6)
    {
        set(PAY_HIDE, [1, 2, 3, 4]);
        pay(" by=other", 7);
    }
    else if (step == 7)
    {
        pay(" by=other", 2);
    }
    else if (step == 8)
    {
        set(5, [PAY_HIDE, PAY_HIDE, PAY_HIDE, PAY_HIDE]);
        pay(" by=other", 7);
    }
    else if (step == 9)
    {
        pay(" by=other", 5);
    }
    else if (step == 10)
    {
        set(PAY_DEFAULT, [PAY_DEFAULT, PAY_DEFAULT, PAY_DEFAULT, PAY_DEFAULT]);
        // The state change is the timer handler's, at its end.
        llSetTimerEvent(0.5);
    }
    else if (step == 12)
    {
        emit("waiting", " drive=answer grant=" + (string)PERMISSION_DEBIT);
        llRequestPermissions(llGetOwner(), PERMISSION_DEBIT);
        llSetTimerEvent(15.0);
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
        if (step == 0)
        {
            begin();
            next();
            return;
        }
        emit("state", " step=" + (string)step + " state=default");
        next();
    }

    money(key id, integer amount)
    {
        emit("money", " script=till step=" + (string)step + " payer=" + who(id)
            + " amount=" + (string)amount);
        heard(id, amount);
    }

    run_time_permissions(integer perms)
    {
        llSetTimerEvent(0.0);
        emit("granted", " step=" + (string)step + " perms=" + (string)perms);
        if (paid > 0 && (perms & PERMISSION_DEBIT))
        {
            integer result = llGiveMoney(other, paid);
            emit("refund", " step=" + (string)step + " amount=" + (string)paid
                + " result=" + (string)result);
        }
        else
        {
            emit("refund", " step=" + (string)step + " amount=0");
        }
        step = 12;
        llSetTimerEvent(8.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (step == 10)
        {
            state closed;
        }
        next();
    }
}

state closed
{
    state_entry()
    {
        emit("state", " step=" + (string)step + " state=closed");
        phase = 0;
        pay(" by=other", 1);
    }

    link_message(integer sender, integer number, string text, key id)
    {
        if (text == "paid")
        {
            heard(id, number);
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (phase == 0)
        {
            phase = 1;
            step = 11;
            llMessageLinked(LINK_THIS, 0, "remove", NULL_KEY);
            pay("", 1);
            return;
        }
        state default;
    }
}
