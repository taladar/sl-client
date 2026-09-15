// `print` is a keyword of the grid's grammar, not a library function — the
// grid's function table never lists it — and it is an expression, so it may
// stand wherever one does, a `for` clause included. Reported as a call to an
// undefined function until the parser modelled it.
default
{
    state_entry()
    {
        integer i;
        print("start");
        for (print(i), i = 0; i < 2; print(i), ++i)
        {
            print(<1.0, 2.0, 3.0>);
        }
    }
}
