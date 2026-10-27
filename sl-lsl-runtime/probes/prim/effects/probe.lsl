// probe: prim/effects
// script: Probe
// order: 2
// drive: none
//
// What the sound, particle and texture-animation calls say about their
// arguments, on each grid. No function reads these back, so each call is a
// `try` record and its answer is the debug-channel chat the runner notes
// beside it, if any. The sound calls with a name not in the prim's inventory,
// a key no asset has, NULL_KEY and an empty string; llPlaySound with a volume
// past 1 and below 0; llSetSoundRadius, llAdjustSoundVolume past their
// ranges, llSetSoundQueueing, llStopSound with nothing playing; and
// llTriggerSoundLimited with a box upside down. llParticleSystem with an
// unknown rule, a value of the wrong type, a
// texture name not in the inventory, and an empty list; llLinkParticleSystem
// of a link that is not there. llSetTextureAnim with zero frames, a negative
// rate, a face past the prim's and the flags without ANIM_ON. And
// llSetPrimMediaParams and llGetPrimMediaParams of a face past the prim's, a
// URL past 1024 characters and an unknown rule. A particle rule missing its
// value is tried by the script in prim B (see there), because OpenSim throws
// on it.
string ID = "prim/effects";

string NOTHING = "11111111-2222-3333-4444-555555555555";

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

attempt(string label)
{
    emit("try", " label=" + label);
}

// Leaves the debug-channel chat of the call before time to arrive before
// the next `try`.
pause()
{
    llSleep(0.5);
}

default
{
    state_entry()
    {
        begin();

        // Sounds.
        attempt("play-missing-name");
        llPlaySound("no such sound", 1.0);
        pause();
        attempt("play-unknown-key");
        llPlaySound(NOTHING, 1.0);
        pause();
        attempt("play-null-key");
        llPlaySound(NULL_KEY, 1.0);
        pause();
        attempt("play-empty");
        llPlaySound("", 1.0);
        pause();
        attempt("play-volume-2");
        llPlaySound(NOTHING, 2.0);
        pause();
        attempt("play-volume-negative");
        llPlaySound(NOTHING, -1.0);
        pause();
        attempt("loop-missing-name");
        llLoopSound("no such sound", 1.0);
        pause();
        attempt("loop-master-missing-name");
        llLoopSoundMaster("no such sound", 1.0);
        pause();
        attempt("loop-slave-missing-name");
        llLoopSoundSlave("no such sound", 1.0);
        pause();
        attempt("trigger-missing-name");
        llTriggerSound("no such sound", 1.0);
        pause();
        attempt("trigger-limited-upside-down");
        vector p = llGetPos();
        llTriggerSoundLimited(NOTHING, 1.0, p - <1.0, 1.0, 1.0>, p + <1.0, 1.0, 1.0>);
        pause();
        attempt("preload-missing-name");
        llPreloadSound("no such sound");
        pause();
        attempt("collision-sound-missing-name");
        llCollisionSound("no such sound", 1.0);
        pause();
        llCollisionSound("", 0.0);
        attempt("sound-radius-negative");
        llSetSoundRadius(-5.0);
        pause();
        attempt("sound-radius-large");
        llSetSoundRadius(1000.0);
        pause();
        attempt("adjust-volume-2");
        llAdjustSoundVolume(2.0);
        pause();
        attempt("sound-queueing");
        llSetSoundQueueing(TRUE);
        llSetSoundQueueing(FALSE);
        pause();
        attempt("stop-nothing-playing");
        llStopSound();
        pause();

        // Particles.
        attempt("particles-unknown-rule");
        llParticleSystem([999, 1.0, PSYS_PART_MAX_AGE, 1.0]);
        pause();
        attempt("particles-wrong-type");
        llParticleSystem([PSYS_PART_MAX_AGE, "one", PSYS_SRC_BURST_RATE, 0.5]);
        pause();
        attempt("particles-missing-texture");
        llParticleSystem([PSYS_SRC_TEXTURE, "no such texture", PSYS_PART_MAX_AGE, 1.0]);
        pause();
        attempt("particles-age-past-range");
        llParticleSystem([PSYS_PART_MAX_AGE, 100.0, PSYS_SRC_MAX_AGE, -1.0,
            PSYS_SRC_BURST_PART_COUNT, 10000]);
        pause();
        attempt("particles-empty");
        llParticleSystem([]);
        pause();
        attempt("link-particles-3");
        llLinkParticleSystem(3, [PSYS_PART_MAX_AGE, 1.0]);
        pause();
        llParticleSystem([]);

        // Texture animation.
        attempt("texture-anim-zero-frames");
        llSetTextureAnim(ANIM_ON | LOOP, ALL_SIDES, 0, 0, 0.0, 0.0, 1.0);
        pause();
        attempt("texture-anim-negative-rate");
        llSetTextureAnim(ANIM_ON | LOOP, ALL_SIDES, 2, 2, 0.0, 4.0, -1.0);
        pause();
        attempt("texture-anim-face-9");
        llSetTextureAnim(ANIM_ON | LOOP, 9, 2, 2, 0.0, 4.0, 1.0);
        pause();
        attempt("texture-anim-without-on");
        llSetTextureAnim(LOOP | SMOOTH, ALL_SIDES, 2, 2, 0.0, 4.0, 1.0);
        pause();
        llSetTextureAnim(FALSE, ALL_SIDES, 0, 0, 0.0, 0.0, 0.0);

        // Media.
        attempt("media-face-9");
        integer status = llSetPrimMediaParams(9, [PRIM_MEDIA_HOME_URL, "http://example.com/"]);
        emit("media", " label=face-9 status=" + (string)status);
        string url = "http://example.com/";
        while (llStringLength(url) < 1100)
        {
            url += "0123456789";
        }
        attempt("media-long-url");
        status = llSetPrimMediaParams(0, [PRIM_MEDIA_HOME_URL, url]);
        emit("media", " label=long-url status=" + (string)status + " length=" + (string)
            llStringLength(llList2String(llGetPrimMediaParams(0, [PRIM_MEDIA_HOME_URL]), 0)));
        attempt("media-unknown-rule");
        status = llSetPrimMediaParams(0, [99, 1]);
        emit("media", " label=unknown-rule status=" + (string)status);
        attempt("media-width-past-max");
        status = llSetPrimMediaParams(0, [PRIM_MEDIA_WIDTH_PIXELS, 5000]);
        emit("media", " label=width-past-max status=" + (string)status + " got="
            + llList2String(llGetPrimMediaParams(0, [PRIM_MEDIA_WIDTH_PIXELS]), 0));
        emit("media", " label=get-face-9 count="
            + (string)llGetListLength(llGetPrimMediaParams(9, [PRIM_MEDIA_HOME_URL])));
        emit("media", " label=get-unknown-rule got="
            + q(llList2CSV(llGetPrimMediaParams(0, [99]))));
        status = llClearPrimMedia(0);
        emit("media", " label=clear status=" + (string)status);
        status = llClearPrimMedia(9);
        emit("media", " label=clear-face-9 status=" + (string)status);

        emit("end", "");
    }
}
