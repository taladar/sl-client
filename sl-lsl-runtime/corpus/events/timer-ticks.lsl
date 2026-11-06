// corpus: a timer fires on its ticks, repeats, and stops when set to zero
// ticks: 30
integer fired;

default
{
    state_entry()
    {
        llSetTimerEvent(0.5);
    }

    timer()
    {
        ++fired;
        print("timer " + (string)fired);
        if (fired == 3)
        {
            llSetTimerEvent(0.0);
        }
    }
}
