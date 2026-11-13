// A pose ball: seats half a metre up, asks its sitter for permission to
// animate them and, granted, swaps the stock sit for its own "Pose".
key gSitter = NULL_KEY;

default
{
    state_entry()
    {
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
        llSetSitText("Pose");
        llSetText("Sit to pose", <1.0, 1.0, 1.0>, 1.0);
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            key sitter = llAvatarOnSitTarget();
            if (sitter != NULL_KEY)
            {
                gSitter = sitter;
                llRequestPermissions(sitter, PERMISSION_TRIGGER_ANIMATION);
            }
            else if (gSitter != NULL_KEY)
            {
                llSay(0, llKey2Name(gSitter) + " stood up");
                gSitter = NULL_KEY;
            }
        }
    }

    run_time_permissions(integer granted)
    {
        if (granted & PERMISSION_TRIGGER_ANIMATION)
        {
            llStopAnimation("sit");
            llStartAnimation("Pose");
            llSay(0, "Posing " + llKey2Name(llGetPermissionsKey()));
        }
    }
}
