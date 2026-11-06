// corpus: OpenSim's own table of strings cast to an integer
// Oracle: OpenSim's LSL_TypesTestLSLInteger (m_stringIntSet), in order.
default
{
    state_entry()
    {
        list cases = ["2", "-2", "0", "1", "-1", "123.9", "999999999", "-99999999",
            "", "aa", "56foo", "42", "42 is the answer", " 42", "42,123,456", "0xff",
            "12345678900000"];
        integer i;
        for (i = 0; i < llGetListLength(cases); ++i)
        {
            string text = llList2String(cases, i);
            print("\"" + text + "\" -> " + (string)((integer)text));
        }
    }
}
