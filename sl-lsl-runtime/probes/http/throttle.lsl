// probe: http/throttle
// drive: none
// unordered: answers
//
// How each grid throttles llHTTPRequest from one prim. The script asks for a
// URL of its own, which every request goes to and which answers each with
// status 200 and "ok", then sends a burst of 30 requests in one event as fast
// as llHTTPRequest returns, and straight after it a burst of five more with
// HTTP_VERBOSE_THROTTLE off. For each burst it prints how many requests
// llHTTPRequest accepted (a key) and refused (NULL_KEY), and the index of the
// first refusal; the runner notes what the grid says on DEBUG_CHANNEL. Fifteen
// seconds later it prints every status the answers came with and how many of
// each, the first body of every status but 200, and how many requests reached
// the URL. Then it waits 25 seconds more, sends one request, and prints
// whether it was accepted and what came back — whether the throttle has let
// go by then.
string ID = "http/throttle";

string url;
integer phase;
integer arrived;
list statuses;
list counts;
list bodies;
key late;

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

burst(string which, integer n, list options)
{
    integer accepted = 0;
    integer refused = 0;
    integer first = -1;
    integer i;
    for (i = 0; i < n; ++i)
    {
        key id = llHTTPRequest(url + "/" + which + (string)i, options, "");
        if (id == NULL_KEY)
        {
            refused = refused + 1;
            if (first < 0)
            {
                first = i;
            }
        }
        else
        {
            accepted = accepted + 1;
        }
    }
    emit("burst", " which=" + which + " sent=" + (string)n
        + " accepted=" + (string)accepted
        + " refused=" + (string)refused
        + " first_refused=" + (string)first);
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
            burst("first", 30, []);
            burst("quiet", 5, [HTTP_VERBOSE_THROTTLE, FALSE]);
            phase = 1;
            llSetTimerEvent(15.0);
            return;
        }
        if (method == URL_REQUEST_DENIED)
        {
            emit("url", " granted=0 reason=" + q(body));
            emit("end", "");
            llSetTimerEvent(0.0);
            return;
        }
        arrived = arrived + 1;
        llHTTPResponse(id, 200, "ok");
    }

    http_response(key id, integer status, list metadata, string body)
    {
        if (id == late && phase == 3)
        {
            emit("late-response", " status=" + (string)status
                + " body=" + q(llGetSubString(body, 0, 59)));
            llSetTimerEvent(0.0);
            llReleaseURL(url);
            emit("end", "");
            phase = 4;
            return;
        }
        integer at = llListFindList(statuses, [status]);
        if (at < 0)
        {
            statuses += [status];
            counts += [1];
            bodies += [llGetSubString(body, 0, 59)];
        }
        else
        {
            counts = llListReplaceList(counts, [llList2Integer(counts, at) + 1], at, at);
        }
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
        if (phase == 1)
        {
            integer i;
            integer n = llGetListLength(statuses);
            for (i = 0; i < n; ++i)
            {
                string fields = " status=" + (string)llList2Integer(statuses, i)
                    + " count=" + (string)llList2Integer(counts, i);
                if (llList2Integer(statuses, i) != 200)
                {
                    fields += " body=" + q(llList2String(bodies, i));
                }
                emit("answers", fields);
            }
            emit("arrived", " count=" + (string)arrived);
            phase = 2;
            llSetTimerEvent(25.0);
            return;
        }
        if (phase == 2)
        {
            late = llHTTPRequest(url + "/late", [], "");
            string request = "set";
            if (late == NULL_KEY)
            {
                request = "null";
            }
            emit("late", " request=" + request);
            phase = 3;
            llSetTimerEvent(15.0);
            if (late == NULL_KEY)
            {
                llSetTimerEvent(0.0);
                llReleaseURL(url);
                emit("end", "");
                phase = 4;
            }
            return;
        }
        if (phase == 3)
        {
            llSetTimerEvent(0.0);
            emit("late-response", " status=none");
            llReleaseURL(url);
            emit("end", "");
            phase = 4;
        }
    }
}
