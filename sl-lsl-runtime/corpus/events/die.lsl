// corpus: llDie ends the script and nothing after it runs
// ticks: 5
default
{
    state_entry()
    {
        print("dying");
        llDie();
        print("never");
    }
}
