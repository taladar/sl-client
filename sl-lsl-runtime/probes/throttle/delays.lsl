// probe: throttle/delays
// drive: none
// vary: seconds, least, most
//
// The forced delay each grid puts on a library call: how long the call
// itself takes, by llGetTime before and after, for every function the
// documentation gives a delay that a probe can call without changing
// anything but its own prim, and for a few it gives none (`llSetColor`,
// `llSetScale`, `llSetLinkPrimitiveParamsFast`, `llRegionSay`) beside them
// as the floor. Each call is made three times, a timer tick apart, and
// printed as the mean, the least and the most of the three: on Second Life
// llGetTime moves in frames (about 0.022 s), so a single call is read to a
// frame. The calls that speak to the owner (`llInstantMessage`, `llDialog`,
// `llTextBox`, `llLoadURL`) are noted by the runner as they arrive; the
// ones that ask the dataserver are answered meanwhile, and the answers are
// counted, not printed. The setters set what the prim already has.
string ID = "throttle/delays";

integer step;
integer round;
float total;
float least;
float most;
integer answers;

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

string ms(float seconds)
{
    return (string)(llRound(seconds * 1000.0) / 1000.0);
}

// Calls function `i` once and returns its name, or "" past the last.
string call(integer i)
{
    if (i == 0)
    {
        llRegionSay(-7400, "floor");
        return "llRegionSay";
    }
    if (i == 1)
    {
        llSetColor(<1.0, 1.0, 1.0>, ALL_SIDES);
        return "llSetColor";
    }
    if (i == 2)
    {
        llSetScale(llGetScale());
        return "llSetScale";
    }
    if (i == 3)
    {
        llSetLinkPrimitiveParamsFast(LINK_THIS, [PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);
        return "llSetLinkPrimitiveParamsFast";
    }
    if (i == 4)
    {
        llSetPos(llGetPos());
        return "llSetPos";
    }
    if (i == 5)
    {
        llSetRot(llGetRot());
        return "llSetRot";
    }
    if (i == 6)
    {
        llSetLocalRot(llGetLocalRot());
        return "llSetLocalRot";
    }
    if (i == 7)
    {
        llSetTexture(TEXTURE_BLANK, ALL_SIDES);
        return "llSetTexture";
    }
    if (i == 8)
    {
        llSetLinkTexture(LINK_THIS, TEXTURE_BLANK, ALL_SIDES);
        return "llSetLinkTexture";
    }
    if (i == 9)
    {
        llScaleTexture(1.0, 1.0, ALL_SIDES);
        return "llScaleTexture";
    }
    if (i == 10)
    {
        llOffsetTexture(0.0, 0.0, ALL_SIDES);
        return "llOffsetTexture";
    }
    if (i == 11)
    {
        llRotateTexture(0.0, ALL_SIDES);
        return "llRotateTexture";
    }
    if (i == 12)
    {
        llSetPrimitiveParams([PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);
        return "llSetPrimitiveParams";
    }
    if (i == 13)
    {
        llSetLinkPrimitiveParams(LINK_THIS, [PRIM_COLOR, ALL_SIDES, <1.0, 1.0, 1.0>, 1.0]);
        return "llSetLinkPrimitiveParams";
    }
    if (i == 14)
    {
        llSetRemoteScriptAccessPin(0);
        return "llSetRemoteScriptAccessPin";
    }
    if (i == 15)
    {
        llAdjustSoundVolume(1.0);
        return "llAdjustSoundVolume";
    }
    if (i == 16)
    {
        llGetRegionFlags();
        return "llGetRegionFlags";
    }
    if (i == 17)
    {
        llModPow(2, 10, 1000);
        return "llModPow";
    }
    if (i == 18)
    {
        llXorBase64Strings("QUJD", "QUJD");
        return "llXorBase64Strings";
    }
    if (i == 19)
    {
        llRequestAgentData(llGetOwner(), DATA_ONLINE);
        return "llRequestAgentData";
    }
    if (i == 20)
    {
        llRequestUsername(llGetOwner());
        return "llRequestUsername";
    }
    if (i == 21)
    {
        llRequestDisplayName(llGetOwner());
        return "llRequestDisplayName";
    }
    if (i == 22)
    {
        llRequestSimulatorData(llGetRegionName(), DATA_SIM_STATUS);
        return "llRequestSimulatorData";
    }
    if (i == 23)
    {
        llGetParcelPrimOwners(llGetPos());
        return "llGetParcelPrimOwners";
    }
    if (i == 24)
    {
        llParcelMediaQuery([PARCEL_MEDIA_COMMAND_URL]);
        return "llParcelMediaQuery";
    }
    if (i == 25)
    {
        llGetPrimMediaParams(0, [PRIM_MEDIA_CURRENT_URL]);
        return "llGetPrimMediaParams";
    }
    if (i == 26)
    {
        llClearPrimMedia(0);
        return "llClearPrimMedia";
    }
    if (i == 27)
    {
        llMapDestination(llGetRegionName(), <128.0, 128.0, 30.0>, ZERO_VECTOR);
        return "llMapDestination";
    }
    if (i == 28)
    {
        llInstantMessage(llGetOwner(), "throttle/delays");
        return "llInstantMessage";
    }
    if (i == 29)
    {
        llDialog(llGetOwner(), "throttle/delays", ["OK"], -7401);
        return "llDialog";
    }
    if (i == 30)
    {
        llTextBox(llGetOwner(), "throttle/delays", -7402);
        return "llTextBox";
    }
    if (i == 31)
    {
        llLoadURL(llGetOwner(), "throttle/delays", "https://example.com/");
        return "llLoadURL";
    }
    return "";
}

default
{
    state_entry()
    {
        begin();
        llSetTimerEvent(0.2);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        float before = llGetTime();
        string name = call(step);
        float took = llGetTime() - before;
        if (name == "")
        {
            emit("answers", " dataserver=" + (string)answers);
            emit("end", "");
            return;
        }
        if (round == 0)
        {
            total = 0.0;
            least = took;
            most = took;
        }
        total += took;
        if (took < least)
        {
            least = took;
        }
        if (took > most)
        {
            most = took;
        }
        ++round;
        if (round == 3)
        {
            emit("delay", " fn=" + name + " seconds=" + ms(total / 3.0)
                + " least=" + ms(least) + " most=" + ms(most));
            round = 0;
            ++step;
        }
        llSetTimerEvent(0.2);
    }

    dataserver(key id, string data)
    {
        ++answers;
    }
}
