// probe: money/debit
// drive: touch, answer, pay
//
// What llGiveMoney and llTransferLindenDollars do, and what the
// transaction_result event says. The other avatar touches first, so the
// script learns its key. Before anything is granted the script calls both
// for L$ 1 to the other avatar and prints what each returns; the runner
// notes any DEBUG_CHANNEL complaint and the balances each avatar is told.
// Then it asks the owner for PERMISSION_DEBIT, which the runner grants, and
// gives the other avatar L$ 1 with each function; then transfers L$ 1 to the
// owner itself, to NULL_KEY, to the prim's own key and to a key that names
// nobody, L$ 0 and L$ -1 to the other avatar, more than any account holds,
// and gives L$ 0 and L$ -1 with llGiveMoney. Then it asks the other avatar
// for PERMISSION_DEBIT, which the runner grants as that avatar, and prints
// what it holds after. Last the other avatar pays the prim back L$ 2, what
// the two gifts gave it. A transfer's step ends at its transaction_result or
// after eight seconds; the result's data is printed with the keys in it put
// as roles.
string ID = "money/debit";

integer step;
key other;
key handle;

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
    if (id == NULL_KEY)
    {
        return "none";
    }
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (id == other)
    {
        return "other";
    }
    if (id == llGetKey())
    {
        return "prim";
    }
    if (id == handle)
    {
        return "handle";
    }
    return "someone";
}

// Whether text is a key's form: 36 characters with the four dashes where a
// key has them. A key's truth is no test: OpenSim takes any text for true.
integer is_key(string text)
{
    return llStringLength(text) == 36 && llGetSubString(text, 8, 8) == "-"
        && llGetSubString(text, 13, 13) == "-" && llGetSubString(text, 18, 18) == "-"
        && llGetSubString(text, 23, 23) == "-";
}

// A transaction_result's data with each item that is a key put as its role.
string roles(string data)
{
    list items = llCSV2List(data);
    integer i;
    for (i = 0; i < llGetListLength(items); i++)
    {
        string item = llList2String(items, i);
        if (is_key(item))
        {
            items = llListReplaceList(items, [who((key)item)], i, i);
        }
    }
    return llList2CSV(items);
}

held(string why)
{
    emit("held", " step=" + (string)step + " why=" + why
        + " perms=" + (string)llGetPermissions()
        + " granter=" + who(llGetPermissionsKey()));
}

give(key to, integer amount)
{
    integer result = llGiveMoney(to, amount);
    emit("give", " step=" + (string)step + " to=" + who(to) + " amount=" + (string)amount
        + " result=" + (string)result);
}

transfer(key to, integer amount)
{
    handle = NULL_KEY;
    handle = llTransferLindenDollars(to, amount);
    emit("transfer", " step=" + (string)step + " to=" + who(to) + " amount=" + (string)amount
        + " handle_null=" + (string)(handle == NULL_KEY));
    llSetTimerEvent(8.0);
}

next()
{
    step = step + 1;
    llSetTimerEvent(0.0);
    if (step == 1)
    {
        give(other, 1);
        transfer(other, 1);
    }
    else if (step == 2)
    {
        emit("waiting", " drive=answer grant=" + (string)PERMISSION_DEBIT);
        llRequestPermissions(llGetOwner(), PERMISSION_DEBIT);
        llSetTimerEvent(15.0);
    }
    else if (step == 3)
    {
        give(other, 1);
        llSetTimerEvent(6.0);
    }
    else if (step == 4)
    {
        transfer(other, 1);
    }
    else if (step == 5)
    {
        transfer(llGetOwner(), 1);
    }
    else if (step == 6)
    {
        transfer(NULL_KEY, 1);
    }
    else if (step == 7)
    {
        transfer(llGetKey(), 1);
    }
    else if (step == 8)
    {
        transfer("8d2c4a4e-0f6b-4b3a-9c1d-5e7f8a9b0c1d", 1);
    }
    else if (step == 9)
    {
        transfer(other, 0);
    }
    else if (step == 10)
    {
        transfer(other, -1);
    }
    else if (step == 11)
    {
        transfer(other, 1000000000);
    }
    else if (step == 12)
    {
        give(other, 0);
        give(other, -1);
        llSetTimerEvent(6.0);
    }
    else if (step == 13)
    {
        emit("waiting", " drive=answer by=other grant=" + (string)PERMISSION_DEBIT);
        llRequestPermissions(other, PERMISSION_DEBIT);
        llSetTimerEvent(15.0);
    }
    else if (step == 14)
    {
        emit("waiting", " drive=pay by=other amount=2");
        llSetTimerEvent(10.0);
    }
    else
    {
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        begin();
        held("fresh");
        step = 0;
        emit("waiting", " drive=touch by=other");
    }

    touch_start(integer count)
    {
        if (step == 0)
        {
            other = llDetectedKey(0);
            emit("touch", " by=" + who(other));
            next();
        }
    }

    run_time_permissions(integer perms)
    {
        emit("answer", " step=" + (string)step + " perms=" + (string)perms);
        held("answer");
        next();
    }

    transaction_result(key id, integer success, string data)
    {
        emit("result", " step=" + (string)step + " id=" + who(id)
            + " success=" + (string)success + " data=" + q(roles(data)));
        if (id == handle && step != 1)
        {
            next();
        }
    }

    money(key id, integer amount)
    {
        emit("money", " step=" + (string)step + " payer=" + who(id) + " amount=" + (string)amount);
    }

    timer()
    {
        if (step == 2 || step == 13)
        {
            held("no-answer");
        }
        else
        {
            emit("timeout", " step=" + (string)step);
        }
        next();
    }
}
