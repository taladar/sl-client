// corpus: a state change runs the old state's exit, then the new one's entry,
// and an event of the old state is not run in the new one
// ticks: 5
// at 2: touch
default
{
    state_entry()
    {
        print("default entry");
    }

    touch_start(integer count)
    {
        print("touched in default");
        state other;
    }

    state_exit()
    {
        print("default exit");
    }
}

state other
{
    state_entry()
    {
        print("other entry");
    }

    touch_end(integer count)
    {
        print("touch_end in other");
    }
}
