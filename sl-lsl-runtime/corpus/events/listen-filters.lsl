// corpus: a listen hears what its filter admits, and llListenRemove ends it
// ticks: 8
// at 2: chat 7 hello
// at 3: chat 8 wrong channel
// at 4: chat 7 stop
// at 5: chat 7 after removal
integer handle;

default
{
    state_entry()
    {
        handle = llListen(7, "", llGetOwner(), "");
    }

    listen(integer channel, string name, key id, string message)
    {
        print("heard " + (string)channel + " " + name + ": " + message);
        if (message == "stop")
        {
            llListenRemove(handle);
        }
    }
}
