// Spins, and once a second steps through four colours, rising a quarter of a
// metre a step and saying which step it is on in its floating text.
list COLOURS = [<1.0, 0.0, 0.0>, <1.0, 1.0, 0.0>, <0.0, 1.0, 0.0>,
    <0.0, 0.0, 1.0>];

vector gHome;
integer gStep;

default
{
    state_entry()
    {
        gHome = llGetPos();
        llTargetOmega(<0.0, 0.0, 1.0>, PI_BY_TWO, 1.0);
        llSetTimerEvent(1.0);
    }

    timer()
    {
        gStep = (gStep + 1) % 4;
        llSetColor(llList2Vector(COLOURS, gStep), ALL_SIDES);
        llSetPos(gHome + <0.0, 0.0, 0.25 * gStep>);
        llSetText("Step " + (string)gStep, <1.0, 1.0, 1.0>, 1.0);
    }
}
