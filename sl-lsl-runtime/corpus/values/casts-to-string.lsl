// corpus: a cast into a string prints Mono's digits
// Oracle: the book's simulator/lsl-engine.md: a float prints seven
// significant digits padded with zeros, (string)vector five decimals, the
// same vector inside a list cast to a string six.
default
{
    state_entry()
    {
        print((string)1.5);
        print((string)123456789.0);
        print((string)0.1);
        print((string)<1, 2, 3>);
        print((string)[<1, 2, 3>]);
        print((string)<0, 0, 0, 1>);
        print((string)[1, 2.5, "three", <4, 5, 6>]);
        print((string)(-0.0));
    }
}
