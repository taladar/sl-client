// corpus: string comparison is exact and case-sensitive, != is 0 or 1
// Oracle: the value model ("byte-wise and case-sensitive") and the book
// ("string != string is 0 or 1").
default
{
    state_entry()
    {
        print((string)("abc" == "abc"));
        print((string)("abc" == "ABC"));
        print((string)("cat" != "cot"));
        print((string)("abc" != "abc"));
        print((string)("abc" == "abc "));
    }
}
