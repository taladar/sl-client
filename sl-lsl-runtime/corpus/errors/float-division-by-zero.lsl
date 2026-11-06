// corpus: float division by zero is a Math Error too
// Oracle: the value model's correction: "float division by zero is a
// run-time Math Error (as are vector / 0.0 and a NaN float quotient)".
default
{
    state_entry()
    {
        float zero = 0.0;
        print("before");
        print((string)(1.0 / zero));
        print("after");
    }
}
