// probe: ossl/available
// drive: none
// library: opensim
// vary: version, map_texture, ip, wallclock, apparent
//
// The OSSL functions a script may call on the local OpenSim, one of each
// way a function comes to be allowed, and what each answers. The grid's
// [OSSL] configuration (osslDefaultEnable.ini) sets OSFunctionThreatLevel
// to VeryLow and names many functions on an Allow_ line, which then
// overrides the threat level both ways; the owner of the probe's prims is
// neither estate owner nor estate manager. So:
//
// - rec=allow-true: functions whose Allow_ line is `true`, most of them
//   above VeryLow (osGetScriptEngineName and osGetSimulatorVersion are
//   High, osGetGridName Moderate) — osGetAgentIP is one too, behind a
//   second check of its own that the owner is a god;
// - rec=threat-level: functions with no Allow_ line at threat level None,
//   under the configured VeryLow;
// - rec=always: functions that check only that OSSL is enabled at all;
// - rec=unchecked: functions that check nothing.
//
// Every call that could throw is in ossl/errors instead. On Second Life
// none of this compiles, and the compile errors are the measurement.
string ID = "ossl/available";

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

default
{
    state_entry()
    {
        begin();
        key owner = llGetOwner();

        emit("allow-true", " fn=osGetScriptEngineName value="
            + q(osGetScriptEngineName()));
        emit("allow-true", " fn=osGetSimulatorVersion version="
            + q(osGetSimulatorVersion()));
        emit("allow-true", " fn=osGetPhysicsEngineType value="
            + q(osGetPhysicsEngineType()));
        emit("allow-true", " fn=osGetGridName value=" + q(osGetGridName()));
        emit("allow-true", " fn=osGetGridNick value=" + q(osGetGridNick()));
        emit("allow-true", " fn=osFormatString value="
            + q(osFormatString("{0}-{1}-{0}", ["a", 2])));
        emit("allow-true", " fn=osMatchString value="
            + q(llList2CSV(osMatchString("abcabc", "b", 0))));
        emit("allow-true", " fn=osReplaceString value="
            + q(osReplaceString("abcabc", "b", "x", -1, 0)));
        emit("allow-true", " fn=osRegexIsMatch value="
            + (string)osRegexIsMatch("abc", "^a.c$"));
        emit("allow-true", " fn=osUnixTimeToTimestamp value="
            + q(osUnixTimeToTimestamp(0)));
        emit("allow-true", " fn=osGetWindParam value="
            + (string)osGetWindParam("SimpleRandomWind", "strength"));
        string ip = osGetAgentIP((string)owner);
        emit("allow-true", " fn=osGetAgentIP ip=" + q(ip)
            + " len=" + (string)llStringLength(ip));
        integer handle = osListenRegex(7, "", NULL_KEY, "^probe [0-9]+$",
            OS_LISTEN_REGEX_MESSAGE);
        emit("allow-true", " fn=osListenRegex handle_positive="
            + (string)(handle > 0));
        llListenRemove(handle);

        emit("threat-level", " fn=osGetGender value="
            + q(osGetGender(owner)));
        emit("threat-level", " fn=osGetHealth value="
            + (string)osGetHealth(owner));
        emit("threat-level", " fn=osGetRezzingObject value="
            + (string)osGetRezzingObject());
        emit("threat-level", " fn=osWindActiveModelPluginName value="
            + q(osWindActiveModelPluginName()));
        emit("threat-level", " fn=osDrawText value="
            + q(osDrawText("", "hi")));

        emit("always", " fn=osGetRegionSize value="
            + q((string)osGetRegionSize()));
        emit("always", " fn=osIsNpc value=" + (string)osIsNpc(owner));
        emit("always", " fn=osIsUUID value=" + (string)osIsUUID("nope")
            + "," + (string)osIsUUID((string)owner));
        emit("always", " fn=osMin value=" + (string)osMin(1.5, -2.0));
        emit("always", " fn=osMax value=" + (string)osMax(1.5, -2.0));
        emit("always", " fn=osGetSunParam value="
            + (string)osGetSunParam("day_length"));
        emit("always", " fn=osGetLinkNumber value="
            + (string)osGetLinkNumber(llGetObjectName()));
        emit("always", " fn=osGetMapTexture map_texture="
            + (string)osGetMapTexture());
        emit("always", " fn=osGetPhysicsEngineName value="
            + q(osGetPhysicsEngineName()));
        emit("always", " fn=osCheckODE value=" + (string)osCheckODE());
        emit("always", " fn=osStringSubString value="
            + q(osStringSubString("abcdef", 2, 3)));
        emit("always", " fn=osGetApparentTimeString apparent="
            + q(osGetApparentTimeString(TRUE)));

        emit("unchecked", " fn=osSHA256 value=" + q(osSHA256("abc")));
        emit("unchecked", " fn=osRound value=" + (string)osRound(1.23456, 2));
        emit("unchecked", " fn=osVecMagSquare value="
            + (string)osVecMagSquare(<1.0, 2.0, 3.0>));
        emit("unchecked", " fn=osApproxEquals value="
            + (string)osApproxEquals(1.0, 1.0000001));
        emit("unchecked", " fn=osStringReplace value="
            + q(osStringReplace("abcabc", "b", "x")));
        emit("unchecked", " fn=osIsNotValidNumber value="
            + (string)osIsNotValidNumber(1.0));
        emit("unchecked", " fn=osGetPSTWallclock wallclock="
            + (string)osGetPSTWallclock());
        emit("unchecked", " fn=osGetInventoryItemKey value="
            + (string)osGetInventoryItemKey("nope"));

        emit("end", "");
    }
}
