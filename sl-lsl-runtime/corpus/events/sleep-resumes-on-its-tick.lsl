// corpus: llSleep suspends the script and it resumes on the tick it is due
// ticks: 25
default
{
    state_entry()
    {
        print("asleep");
        llSleep(1.0);
        print("awake");
        llSleep(0.5);
        print("again");
    }
}
