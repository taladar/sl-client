// probe: throttle/http-window
// drive: none
// vary: pattern, accepted, first_after, seconds, at
//
// How llHTTPRequest's throttle lets go. `http/throttle` found the burst each
// grid accepts (25 on Second Life, 4 on OpenSim) and that a request 40
// seconds later is accepted again; this probe measures the time between.
// The script asks for a URL of its own, which answers every request with
// 200, sends requests in a row until one is refused (at most 40), then one
// request every half second for forty seconds, quietly
// (HTTP_VERBOSE_THROTTLE off), and prints the outcome of each as a pattern,
// `A` accepted and `r` refused, with how many seconds after the burst the
// first was accepted and how many were accepted in all. Then it waits in
// silence, five seconds, then ten, fifteen, twenty and twenty-five, sending
// one request after each wait, and prints whether each was accepted; after
// the first that is, it sends requests in a row until one is refused (at most
// 40) and prints how many were accepted: what the quiet refilled.
string ID = "throttle/http-window";

string url;
integer phase;
integer ticks;
integer gap;
float phase_start;
string pattern;
integer accepted;
float burst_end;
float first_after = -1.0;

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
        llRequestURL();
        llSetTimerEvent(20.0);
    }

    http_request(key id, string method, string body)
    {
        if (method == URL_REQUEST_GRANTED)
        {
            url = body;
            emit("url", " granted=1");
            integer sent = 0;
            integer refused = 0;
            while (sent < 40 && !refused)
            {
                key request = llHTTPRequest(url + "/burst" + (string)sent,
                    [HTTP_VERBOSE_THROTTLE, FALSE], "");
                ++sent;
                if (request == NULL_KEY)
                {
                    refused = 1;
                }
            }
            emit("burst", " sent=" + (string)sent + " refused=" + (string)refused);
            burst_end = llGetTime();
            phase = 1;
            llSetTimerEvent(0.5);
            return;
        }
        if (method == URL_REQUEST_DENIED)
        {
            emit("url", " granted=0 reason=" + q(body));
            emit("end", "");
            llSetTimerEvent(0.0);
            return;
        }
        llHTTPResponse(id, 200, "ok");
    }

    timer()
    {
        if (phase == 0)
        {
            llSetTimerEvent(0.0);
            emit("url", " granted=none");
            emit("end", "");
            return;
        }
        if (phase == 2)
        {
            llSetTimerEvent(0.0);
            key quiet = llHTTPRequest(url + "/quiet" + (string)gap,
                [HTTP_VERBOSE_THROTTLE, FALSE], "");
            integer ok = quiet != NULL_KEY;
            emit("quiet", " waited=" + (string)(gap * 5) + " accepted=" + (string)ok
                + " at=" + (string)(llRound((llGetTime() - phase_start) * 10.0) / 10.0));
            if (ok)
            {
                integer refill = 1;
                integer sent = 1;
                integer refused = 0;
                while (sent < 40 && !refused)
                {
                    key more = llHTTPRequest(url + "/refill" + (string)sent,
                        [HTTP_VERBOSE_THROTTLE, FALSE], "");
                    ++sent;
                    if (more == NULL_KEY)
                    {
                        refused = 1;
                    }
                    else
                    {
                        ++refill;
                    }
                }
                emit("refill", " accepted=" + (string)refill + " refused=" + (string)refused);
            }
            if (ok || gap == 5)
            {
                phase = 3;
                llReleaseURL(url);
                emit("end", "");
                return;
            }
            ++gap;
            phase_start = llGetTime();
            llSetTimerEvent(gap * 5.0);
            return;
        }
        if (phase != 1)
        {
            return;
        }
        key request = llHTTPRequest(url + "/tick" + (string)ticks,
            [HTTP_VERBOSE_THROTTLE, FALSE], "");
        ++ticks;
        if (request == NULL_KEY)
        {
            pattern += "r";
        }
        else
        {
            pattern += "A";
            ++accepted;
            if (first_after < 0.0)
            {
                first_after = llGetTime() - burst_end;
            }
        }
        if (ticks < 80)
        {
            return;
        }
        llSetTimerEvent(0.0);
        string first = "none";
        if (first_after >= 0.0)
        {
            first = (string)(llRound(first_after * 10.0) / 10.0);
        }
        emit("window", " ticks=80 every=0.5 accepted=" + (string)accepted
            + " first_after=" + first
            + " seconds=" + (string)(llRound((llGetTime() - burst_end) * 10.0) / 10.0)
            + " pattern=" + q(pattern));
        phase = 2;
        phase_start = llGetTime();
        gap = 1;
        llSetTimerEvent(5.0);
    }
}
