// Answers "hello" in local chat, and takes commands on channel 42: "spin"
// and "stop".
default
{
    state_entry()
    {
        llSetText("Say hello, or /42 spin or /42 stop", <1.0, 1.0, 1.0>, 1.0);
        llListen(0, "", NULL_KEY, "hello");
        llListen(42, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (channel == 0)
        {
            llSay(0, "Hello, " + name + "!");
        }
        else if (message == "spin")
        {
            llTargetOmega(<0.0, 0.0, 1.0>, PI, 1.0);
            llSay(0, "Spinning for " + name);
        }
        else if (message == "stop")
        {
            llTargetOmega(ZERO_VECTOR, 0.0, 0.0);
            llSay(0, "Stopped for " + name);
        }
        else
        {
            llSay(0, "No such command: " + message);
        }
    }
}
