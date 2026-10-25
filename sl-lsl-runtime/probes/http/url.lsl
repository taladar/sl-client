// probe: http/url
// drive: fetch
// vary: free
//
// In-world URLs: what each grid hands out, what they cost and how a request
// from outside the grid reaches a script and comes back. The script prints
// llGetFreeURLs, asks for a URL (llRequestURL), a secure one
// (llRequestSecureURL) and a third, printing for each whether it was granted,
// whether the http_request's id is the key the request returned, the URL's
// scheme, port and path (the key in it is a placeholder in a comparison),
// whether its host is a name or an address, and how many URLs it took from
// llGetFreeURLs. It releases the secure one (the third if the secure one was
// denied) and prints what llGetFreeURLs gave back, then and two seconds
// later.
//
// Then the runner sends requests to the first URL from outside the grid, one
// `fetch` each: a GET with a path, a query and a header of its own, a POST of
// 3000 characters, PUT, DELETE, PATCH, HEAD and OPTIONS, and requests whose
// answer is the measurement — 5000 characters, CONTENT_TYPE_HTML, the status
// 418, and none at all (the runner notes the status the grid sends, and when,
// in its place). For each the script prints the method, the path and query
// headers, the length and start of the body, the request's other headers as
// llGetHTTPHeader gives them (the caller's address only as `set` or not), and
// what it answers. Last it releases that URL and the runner asks once more.
// A step the script hears nothing of within eight seconds prints
// `no-inbound`; the one whose answer is left out waits forty.
string ID = "http/url";

string url;
string secure;
string third;
integer free0;
integer phase;
integer step;
key asked;
string asking;

list METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS",
    "GET", "GET", "GET", "GET"];
list PATHS = ["/get/a?x=1&y=two%20words", "/post", "/put", "/delete", "/patch",
    "/head", "/options", "/big", "/html", "/status", "/silent"];

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

string clip(string s)
{
    if (llStringLength(s) > 40)
    {
        return q(llGetSubString(s, 0, 39) + "...");
    }
    return q(s);
}

string fill(integer n)
{
    string s = "0123456789";
    while (llStringLength(s) < n)
    {
        s += s;
    }
    return llGetSubString(s, 0, n - 1);
}

// A path without the slashes it starts with: on a grid whose URLs end in
// one, "<url>/post" arrives as "//post".
string bare(string path)
{
    while (llGetSubString(path, 0, 0) == "/")
    {
        path = llGetSubString(path, 1, -1);
    }
    return path;
}

// How x-script-url compares with the URL the script was given.
string script_url(string value)
{
    if (value == "")
    {
        return "empty";
    }
    if (value == url)
    {
        return "match";
    }
    if (value + "/" == url)
    {
        return "without-slash";
    }
    return "other";
}

string set_or_empty(string value)
{
    if (value == "")
    {
        return "empty";
    }
    return "set";
}

// A URL's scheme, port and path, and whether its host is a name or an
// address.
string shape(string u)
{
    integer colon = llSubStringIndex(u, "://");
    string scheme = llGetSubString(u, 0, colon - 1);
    string rest = llGetSubString(u, colon + 3, -1);
    integer slash = llSubStringIndex(rest, "/");
    string authority = rest;
    string path = "";
    if (slash >= 0)
    {
        authority = llGetSubString(rest, 0, slash - 1);
        path = llGetSubString(rest, slash, -1);
    }
    string host = authority;
    string port = "none";
    integer at = llSubStringIndex(authority, ":");
    if (at >= 0)
    {
        host = llGetSubString(authority, 0, at - 1);
        port = llGetSubString(authority, at + 1, -1);
    }
    string kind = "address";
    integer i;
    for (i = 0; i < llStringLength(host); ++i)
    {
        if (llSubStringIndex("0123456789.", llGetSubString(host, i, i)) < 0)
        {
            kind = "name";
        }
    }
    return " scheme=" + scheme + " port=" + port + " path=" + q(path)
        + " host=" + kind + " length=" + (string)llStringLength(u);
}

ask(string kind)
{
    asking = kind;
    if (kind == "secure")
    {
        asked = llRequestSecureURL();
    }
    else
    {
        asked = llRequestURL();
    }
    string request = "set";
    if (asked == NULL_KEY)
    {
        request = "null";
    }
    emit("asked", " kind=" + kind + " request=" + request);
    llSetTimerEvent(20.0);
}

answered(key id, string kind, integer granted, string body)
{
    string fields = " kind=" + kind + " granted=" + (string)granted
        + " id_matches=" + (string)(id == asked)
        + " used=" + (string)(free0 - llGetFreeURLs())
        + " free=" + (string)llGetFreeURLs();
    if (granted)
    {
        fields += shape(body);
    }
    else
    {
        fields += " reason=" + q(body);
    }
    emit("granted", fields);
}

// Prints the next `fetch` the runner is to send; returns FALSE when none is
// left.
integer fetch()
{
    if (step >= llGetListLength(PATHS))
    {
        return FALSE;
    }
    string extra = "";
    if (step == 0)
    {
        extra = " header=\"X-Probe: one\"";
    }
    else if (step == 1)
    {
        extra = " body=x repeat=3000 type=\"text/plain\"";
    }
    else if (step == 2 || step == 4)
    {
        extra = " body=changed type=\"text/plain\"";
    }
    emit("waiting", " drive=fetch url=" + url
        + " method=" + llList2String(METHODS, step)
        + " path=" + q(llList2String(PATHS, step)) + extra);
    llSetTimerEvent(8.0);
    return TRUE;
}

after_release()
{
    phase = 4;
    llReleaseURL(url);
    emit("released", " kind=plain used=" + (string)(free0 - llGetFreeURLs()));
    emit("waiting", " drive=fetch url=" + url + " method=GET path=\"/after-release\"");
    llSetTimerEvent(8.0);
}

default
{
    state_entry()
    {
        begin();
        free0 = llGetFreeURLs();
        emit("free", " when=before free=" + (string)free0);
        ask("plain");
    }

    http_request(key id, string method, string body)
    {
        if (method == URL_REQUEST_GRANTED || method == URL_REQUEST_DENIED)
        {
            integer granted = method == URL_REQUEST_GRANTED;
            llSetTimerEvent(0.0);
            answered(id, asking, granted, body);
            if (asking == "plain")
            {
                url = body;
                ask("secure");
            }
            else if (asking == "secure")
            {
                if (granted)
                {
                    secure = body;
                }
                ask("third");
            }
            else
            {
                if (granted)
                {
                    third = body;
                }
                string gone = "secure";
                if (secure != "")
                {
                    llReleaseURL(secure);
                }
                else
                {
                    gone = "third";
                    llReleaseURL(third);
                    third = "";
                }
                emit("released", " kind=" + gone + " used=" + (string)(free0 - llGetFreeURLs()));
                phase = 1;
                llSetTimerEvent(2.0);
            }
            return;
        }
        string path = llGetHTTPHeader(id, "x-path-info");
        string name = bare(path);
        emit("inbound", " method=" + q(method)
            + " path=" + q(path)
            + " query=" + q(llGetHTTPHeader(id, "x-query-string"))
            + " length=" + (string)llStringLength(body)
            + " body=" + clip(body)
            + " script_url=" + script_url(llGetHTTPHeader(id, "x-script-url"))
            + " remote_ip=" + set_or_empty(llGetHTTPHeader(id, "x-remote-ip"))
            + " user_agent=" + q(llGetHTTPHeader(id, "user-agent"))
            + " content_type=" + q(llGetHTTPHeader(id, "content-type"))
            + " content_length=" + q(llGetHTTPHeader(id, "content-length"))
            + " x_probe=" + q(llGetHTTPHeader(id, "x-probe"))
            + " host=" + set_or_empty(llGetHTTPHeader(id, "host"))
            + " shard=" + q(llGetHTTPHeader(id, "x-secondlife-shard")));
        if (name == "silent")
        {
            // The next request waits until the grid has given this one up:
            // releasing the URL meanwhile could end it otherwise.
            emit("answer", " path=" + q(path) + " status=none");
            step = step + 1;
            phase = 3;
            llSetTimerEvent(40.0);
            return;
        }
        integer status = 200;
        string answer = "ok";
        if (name == "post")
        {
            status = 201;
            llSetContentType(id, CONTENT_TYPE_JSON);
            answer = "{\"ok\":1}";
        }
        else if (name == "big")
        {
            answer = fill(5000);
        }
        else if (name == "html")
        {
            llSetContentType(id, CONTENT_TYPE_HTML);
            answer = "<p>hi</p>";
        }
        else if (name == "status")
        {
            status = 418;
            answer = "teapot";
        }
        emit("answer", " path=" + q(path) + " status=" + (string)status
            + " length=" + (string)llStringLength(answer));
        llHTTPResponse(id, status, answer);
        if (phase == 2)
        {
            step = step + 1;
            llSetTimerEvent(1.0);
            phase = 3;
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (phase == 0)
        {
            emit("granted", " kind=" + asking + " granted=none");
            emit("end", "");
            return;
        }
        if (phase == 1)
        {
            emit("free", " when=later used=" + (string)(free0 - llGetFreeURLs()));
            if (url == "")
            {
                emit("end", "");
                return;
            }
            phase = 2;
            fetch();
            return;
        }
        if (phase == 2)
        {
            emit("no-inbound", " path=" + q(llList2String(PATHS, step)));
            step = step + 1;
            phase = 3;
        }
        if (phase == 3)
        {
            phase = 2;
            if (!fetch())
            {
                after_release();
            }
            return;
        }
        if (phase == 4)
        {
            if (third != "")
            {
                llReleaseURL(third);
            }
            emit("free", " when=after used=" + (string)(free0 - llGetFreeURLs()));
            emit("end", "");
        }
    }
}
