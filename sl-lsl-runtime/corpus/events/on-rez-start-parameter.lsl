// corpus: on_rez brings the start parameter, which llGetStartParameter keeps
// across a reset that puts every global back to its initial value
// ticks: 6
// at 2: rez 99
integer resets;

default
{
    state_entry()
    {
        print("entry, start parameter " + (string)llGetStartParameter() + ", resets " + (string)resets);
    }

    on_rez(integer param)
    {
        print("on_rez " + (string)param);
        resets = 5;
        llResetScript();
    }
}
