// corpus: integer arithmetic is 32-bit and wraps; it never saturates
integer biggest = 2147483647;
integer wrapped;
integer negated;

default
{
    state_entry()
    {
        wrapped = biggest + 1;
        // -(-2147483648) is itself: there is no positive 2147483648.
        negated = -wrapped;
        print((string)wrapped);
        print((string)negated);
        print((string)(biggest * 2));
    }
}
