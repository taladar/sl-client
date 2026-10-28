// probe: throttle/email
// script: Sender
// order: 2
// drive: none
// vary: delay
//
// llEmail from one object to another in the same region, and its forced
// delay. Prim A asks on channel -7410 for the Mailbox's key in prim B (see
// there), then sends it two emails: one to `<key>@lsl.secondlife.com`, the
// address the documentation gives an object, and one to the grid's own
// domain for objects (`lsl.aditi.lindenlab.com` on Second Life's beta grid,
// OpenSim's default `lsl.opensim.local`), each with a subject naming the
// domain and a two-line body. It prints how long each llEmail call took, and
// tells the Mailbox when it sent, which prints what arrives. A minute after
// the second send it ends. What a grid says about a call goes to
// DEBUG_CHANNEL, which the runner notes.
string ID = "throttle/email";

integer CHANNEL = -7410;
integer phase;
key mailbox;
integer waited;

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

send(string subject, string domain)
{
    llRegionSayTo(mailbox, CHANNEL, "sent|" + subject + "|" + (string)llGetUnixTime());
    float before = llGetTime();
    llEmail((string)mailbox + "@" + domain, subject, "first line\nlast line");
    emit("sent", " subject=" + q(subject) + " domain=" + q(domain)
        + " delay=" + (string)(llRound((llGetTime() - before) * 1000.0) / 1000.0));
}

default
{
    state_entry()
    {
        begin();
        llListen(CHANNEL, "", NULL_KEY, "");
        llRegionSay(CHANNEL, "where");
        llSetTimerEvent(2.0);
    }

    listen(integer channel, string name, key id, string text)
    {
        if (phase != 0 || llGetSubString(text, 0, 4) != "here|")
        {
            return;
        }
        phase = 1;
        mailbox = (key)llGetSubString(text, 5, -1);
        emit("mailbox", " found=1");
        llSetTimerEvent(0.0);
        send("secondlife", "lsl.secondlife.com");
        string own = "lsl.aditi.lindenlab.com";
        if (llSubStringIndex(llGetEnv("sim_channel"), "OpenSim") >= 0)
        {
            own = "lsl.opensim.local";
        }
        send("own", own);
        phase = 2;
        llSetTimerEvent(60.0);
    }

    timer()
    {
        if (phase == 0)
        {
            ++waited;
            if (waited < 10)
            {
                llRegionSay(CHANNEL, "where");
                return;
            }
            emit("mailbox", " found=0");
        }
        llSetTimerEvent(0.0);
        emit("end", "");
        phase = 3;
    }
}
