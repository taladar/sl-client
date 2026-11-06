// corpus: a link message carries its sender, number, string and key
// ticks: 4
// at 2: link 42 some text 0a0a0a0a-0000-4000-8000-000000000001
default
{
    link_message(integer sender, integer num, string text, key id)
    {
        print((string)sender + " " + (string)num + " [" + text + "] " + (string)id);
    }
}
