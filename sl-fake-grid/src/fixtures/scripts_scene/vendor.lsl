// Clicked to pay, offers L$50 or L$100 and thanks whoever pays it.
default
{
    state_entry()
    {
        llSetClickAction(CLICK_ACTION_PAY);
        llSetPayPrice(50, [50, 100, PAY_HIDE, PAY_HIDE]);
        llSetText("Pay me", <1.0, 1.0, 1.0>, 1.0);
    }

    money(key payer, integer amount)
    {
        llSay(0, "Thank you " + llKey2Name(payer) + " for L$"
            + (string)amount);
    }
}
