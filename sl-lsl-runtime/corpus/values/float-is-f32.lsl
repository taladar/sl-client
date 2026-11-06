// corpus: a stored float is 32 bits and drifts, but within one expression
// Mono keeps the double until something stores it
// Oracle: the value model ("float is f32 -- a script that adds 0.1 a hundred
// times sees f32 drift"); where Mono rounds, the aditi transcripts of
// math/float-rounding that vm/tests/precision.rs replays.
float total;
float big = 16777216.0;
float stored;

default
{
    state_entry()
    {
        integer i;
        for (i = 0; i < 100; ++i)
        {
            total += 0.1;
        }
        print((string)(total == 10.0));
        // 2^24 + 1 is no float: stored, the sum rounds back to 2^24 ...
        stored = big + 1.0;
        print((string)(stored == big));
        // ... but compared in the same expression it is still 2^24 + 1.
        print((string)(big + 1.0 == big));
    }
}
