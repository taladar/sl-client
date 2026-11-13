// The rezzer's child: says the start parameter it was rezzed with, and dies
// five seconds later.
default
{
    on_rez(integer start)
    {
        llSay(0, "Spark " + (string)start + " lit");
        llSetTimerEvent(5.0);
    }

    timer()
    {
        llDie();
    }
}
