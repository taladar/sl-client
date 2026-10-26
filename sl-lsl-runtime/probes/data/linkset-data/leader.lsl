// probe: data/linkset-data
// script: Leader
// order: 1
// drive: none
//
// The linkset data store: its budget, what each call costs and answers, and
// the linkset_data event. Prim A holds this script and the Watcher, which
// says by link message when it is ready and keeps every linkset_data event
// it gets until it is asked to print them; prim B, an object of its own,
// holds the Stranger, which says on channel 7 when it is ready and prints
// what it sees of the store when it hears "look".
//
// Once both are ready, a second apart, this script: prints the empty store
// (llLinksetDataAvailable, llLinksetDataCountKeys); writes pairs of chosen
// lengths, ASCII and two-byte characters, the same value again, an empty
// value, an empty key and a key of 1100 characters, printing each write's
// answer and how much llLinksetDataAvailable went down; writes protected
// pairs with a password of 2, 0 and 20 characters and reads, overwrites and
// deletes them with no, a wrong and the right password; lists, finds and
// counts keys with llLinksetDataListKeys, llLinksetDataFindKeys and
// llLinksetDataCountFound by start and count, by pattern and by a pattern
// that is no regular expression; deletes by pattern with
// llLinksetDataDeleteFound with and without the password of a protected
// pair among them; has the Stranger look; fills the store with pairs of
// 1000 characters until a write is refused, then writes what is left;
// resets it with llLinksetDataReset; writes two pairs and resets this
// script, then prints what is still there. Every linkset_data event this
// script gets is printed as it comes (`event`), except while it fills the
// store, where it only counts them (its own link message telling the
// Watcher takes a place in its queue too); last the Watcher prints its own.
string ID = "data/linkset-data";

integer step;
integer watcher;
integer stranger;
integer before;
integer filling;
integer fill_events;
list ACTIONS = ["reset", "update", "delete", "multidelete"];

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

string fill(string c, integer n)
{
    string s = c;
    while (llStringLength(s) < n)
    {
        s += s;
    }
    return llGetSubString(s, 0, n - 1);
}

// How much llLinksetDataAvailable went down since the last call.
string cost()
{
    integer now = llLinksetDataAvailable();
    string out = " cost=" + (string)(before - now) + " available=" + (string)now;
    before = now;
    return out;
}

// A string as a record shows it: quoted, or only its length when it is
// longer than 40 characters.
string shorten(string s)
{
    if (llStringLength(s) > 40)
    {
        return "length:" + (string)llStringLength(s);
    }
    return q(s);
}

string action(integer a)
{
    if (a >= 0 && a < llGetListLength(ACTIONS))
    {
        return llList2String(ACTIONS, a);
    }
    return (string)a;
}

write(string label, string name, string value)
{
    integer code = llLinksetDataWrite(name, value);
    emit("write", " case=" + label + " name_length=" + (string)llStringLength(name)
        + " value_length=" + (string)llStringLength(value)
        + " code=" + (string)code + cost());
}

protect(string label, string name, string value, string pass)
{
    integer code = llLinksetDataWriteProtected(name, value, pass);
    emit("write-protected", " case=" + label
        + " name_length=" + (string)llStringLength(name)
        + " value_length=" + (string)llStringLength(value)
        + " pass_length=" + (string)llStringLength(pass)
        + " code=" + (string)code + cost());
}

read(string label, string name)
{
    emit("read", " case=" + label + " value=" + q(llLinksetDataRead(name)));
}

read_protected(string label, string name, string pass)
{
    emit("read-protected", " case=" + label
        + " value=" + q(llLinksetDataReadProtected(name, pass)));
}

remove(string label, string name)
{
    integer code = llLinksetDataDelete(name);
    emit("delete", " case=" + label + " code=" + (string)code + cost());
}

remove_protected(string label, string name, string pass)
{
    integer code = llLinksetDataDeleteProtected(name, pass);
    emit("delete-protected", " case=" + label + " code=" + (string)code + cost());
}

list_keys(integer start, integer count)
{
    emit("list-keys", " start=" + (string)start + " count=" + (string)count
        + " keys=" + q(llDumpList2String(llLinksetDataListKeys(start, count), ",")));
}

find_keys(string pattern, integer start, integer count)
{
    emit("find-keys", " pattern=" + q(pattern) + " start=" + (string)start
        + " count=" + (string)count
        + " keys=" + q(llDumpList2String(llLinksetDataFindKeys(pattern, start, count), ","))
        + " found=" + (string)llLinksetDataCountFound(pattern));
}

delete_found(string label, string pattern, string pass)
{
    list result = llLinksetDataDeleteFound(pattern, pass);
    emit("delete-found", " case=" + label + " pattern=" + q(pattern)
        + " length=" + (string)llGetListLength(result)
        + " result=" + q(llList2CSV(result))
        + " keys=" + (string)llLinksetDataCountKeys() + cost());
}

next()
{
    step = step + 1;
    if (step == 1)
    {
        before = llLinksetDataAvailable();
        emit("empty", " available=" + (string)before
            + " keys=" + (string)llLinksetDataCountKeys()
            + " read=" + q(llLinksetDataRead("a")));
        write("one-one", "a", "1");
        write("ten-hundred", "abcdefghij", fill("x", 100));
        write("same-value", "a", "1");
        write("longer-value", "a", "22");
        write("shorter-value", "a", "3");
        write("two-byte", "u", fill(llUnescapeURL("%C3%A9"), 10));
        write("two-byte-name", fill(llUnescapeURL("%C3%A9"), 10), "v");
        write("empty-new", "e", "");
        write("empty-existing", "a", "");
        write("empty-name", "", "x");
        write("long-name", fill("n", 1100), "x");
        read("after-empty", "a");
        emit("keys", " count=" + (string)llLinksetDataCountKeys());
    }
    else if (step == 2)
    {
        protect("two", "p", "v", "pw");
        protect("empty-pass", "p0", "v", "");
        protect("twenty", "p20", "v", fill("w", 20));
        read("protected-plain", "p");
        read("empty-pass-plain", "p0");
        read_protected("wrong", "p", "nope");
        read_protected("right", "p", "pw");
        read_protected("plain-pair", "abcdefghij", "pw");
        read_protected("missing", "zz", "pw");
        write("over-protected", "p", "new");
        protect("wrong-pass", "p", "new", "nope");
        protect("right-pass", "p", "new", "pw");
        protect("new-pass", "p", "newer", "other");
        read_protected("after-new-pass", "p", "pw");
        protect("protect-plain-pair", "abcdefghij", "y", "pw");
        remove("protected", "p");
        remove_protected("wrong", "p", "nope");
        remove_protected("right", "p", "pw");
        remove("missing", "zz");
        remove_protected("plain-pair", "u", "pw");
        remove("plain", "u");
    }
    else if (step == 3)
    {
        llLinksetDataReset();
        before = llLinksetDataAvailable();
        write("k3", "k3", "v");
        write("k1", "k1", "v");
        write("k10", "k10", "v");
        write("K0", "K0", "v");
        write("k2", "k2", "v");
        protect("k4", "k4", "v", "pw");
        list_keys(0, 0);
        list_keys(0, 2);
        list_keys(2, 10);
        list_keys(10, 1);
        list_keys(-2, 2);
        list_keys(0, -1);
        find_keys("^k[0-9]$", 0, 0);
        find_keys("k", 1, 2);
        find_keys("^K", 0, 0);
        find_keys("", 0, 0);
        find_keys("(", 0, 0);
        emit("keys", " count=" + (string)llLinksetDataCountKeys());
    }
    else if (step == 4)
    {
        delete_found("no-pass", "^k", "");
        delete_found("pass", "^k", "pw");
        delete_found("nothing", "^zz", "");
        delete_found("bad-pattern", "(", "");
    }
    else if (step == 5)
    {
        write("for-stranger", "seen", "by-a");
        llRegionSay(7, "look");
    }
    else if (step == 6)
    {
        emit("stranger-pair", " read=" + q(llLinksetDataRead("own"))
            + " keys=" + (string)llLinksetDataCountKeys());
        llLinksetDataReset();
        before = llLinksetDataAvailable();
        emit("filling", " available=" + (string)before);
        llMessageLinked(LINK_THIS, 3, "filling", "");
        filling = TRUE;
        string value = fill("f", 1000);
        integer n = 0;
        integer code = 0;
        while (code == 0 && n < 200)
        {
            code = llLinksetDataWrite("f" + (string)n, value);
            n = n + 1;
        }
        integer left = llLinksetDataAvailable();
        emit("filled", " writes=" + (string)n + " last_code=" + (string)code
            + " available=" + (string)left + " keys=" + (string)llLinksetDataCountKeys());
        if (left > 2)
        {
            integer fits = llLinksetDataWrite("z", fill("z", left - 1));
            emit("rest", " value_length=" + (string)(left - 1)
                + " code=" + (string)fits
                + " available=" + (string)llLinksetDataAvailable());
            integer over = llLinksetDataWrite("y", "1");
            emit("over", " code=" + (string)over
                + " available=" + (string)llLinksetDataAvailable());
        }
        llMessageLinked(LINK_THIS, 4, "filled", "");
    }
    else if (step == 7)
    {
        emit("fill-events", " count=" + (string)fill_events);
        filling = FALSE;
        llLinksetDataReset();
        emit("reset", " available=" + (string)llLinksetDataAvailable()
            + " keys=" + (string)llLinksetDataCountKeys());
    }
    else if (step == 8)
    {
        llLinksetDataWrite("keep", "x");
        llLinksetDataWrite("phase", "after-reset");
        emit("resetting", "");
        llResetScript();
    }
    else if (step == 9)
    {
        llMessageLinked(LINK_THIS, 2, "report", "");
        llSetTimerEvent(3.0);
        return;
    }
    else
    {
        emit("end", "");
        llSetTimerEvent(0.0);
        return;
    }
    llSetTimerEvent(1.0);
}

default
{
    state_entry()
    {
        if (llLinksetDataRead("phase") == "after-reset")
        {
            emit("after-script-reset", " keep=" + q(llLinksetDataRead("keep"))
                + " keys=" + (string)llLinksetDataCountKeys()
                + " available=" + (string)llLinksetDataAvailable());
            step = 8;
            llSetTimerEvent(1.0);
            return;
        }
        begin();
        llListen(7, "", NULL_KEY, "ready");
        llSetTimerEvent(30.0);
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num == 1 && text == "ready")
        {
            watcher = TRUE;
            if (stranger && step == 0)
            {
                next();
            }
        }
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llGetOwnerKey(id) == llGetOwner())
        {
            stranger = TRUE;
            if (watcher && step == 0)
            {
                next();
            }
        }
    }

    linkset_data(integer act, string name, string value)
    {
        if (filling)
        {
            fill_events = fill_events + 1;
            return;
        }
        emit("event", " action=" + action(act) + " name=" + shorten(name)
            + " value=" + shorten(value));
    }

    timer()
    {
        if (step == 0)
        {
            llSetTimerEvent(0.0);
            emit("ready", " watcher=" + (string)watcher
                + " stranger=" + (string)stranger);
            emit("end", "");
            return;
        }
        next();
    }
}
