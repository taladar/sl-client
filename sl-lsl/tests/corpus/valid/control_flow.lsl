// Every control-flow keyword plus a jump to a later label — order-insensitive
// label resolution means the forward jump is legal and must not be flagged.
integer classify(integer n)
{
    if (n < 0)
    {
        return -1;
    }
    else if (n == 0)
    {
        return 0;
    }

    integer i = 0;
    integer sum = 0;
    while (i < n)
    {
        sum += i;
        ++i;
    }

    do
    {
        --sum;
    }
    while (sum > n);

    if (sum < 0)
    {
        jump done;
    }
    sum = 0;

    @done;
    return sum;
}

default
{
    state_entry()
    {
        llSay(0, (string)classify(4));
    }
}
