// Dumps what OpenSim's culture-sensitive string search reads from Mono, so
// `sl-lsl-runtime`'s port of it (`src/library/collation.rs`) can be built
// from and held to the runtime OpenSim runs on.
//
//   mcs -nologo dump_collation.cs && mono dump_collation.exe tables .
//       writes collation.core.bin (Mono's own collation table, copied out
//       of mscorlib.dll) and culture-en-us.txt (en-US's lower-case map, the
//       code units .NET counts unassigned, and the collator's contractions)
//   mono dump_collation.exe cases collation-cases.txt <seed> <count>
//       writes <count> random searches and every answer .NET gives them
//
// Run it with the Mono OpenSim runs on (`mono --version`; the tables in
// this directory are Mono 6.14.1's), from this directory.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Threading;

class DumpCollation
{
    static readonly CultureInfo EnUs = new CultureInfo("en-US", false);

    static int Main(string[] args)
    {
        Thread.CurrentThread.CurrentCulture = EnUs;
        CultureInfo.DefaultThreadCurrentCulture = EnUs;
        if (args.Length == 2 && args[0] == "tables")
            return Tables(args[1]);
        if (args.Length == 4 && args[0] == "cases")
            return Cases(args[1], int.Parse(args[2]), int.Parse(args[3]));
        Console.Error.WriteLine("usage: tables <dir> | cases <file> <seed> <count>");
        return 2;
    }

    static string Hex(int unit)
    {
        return unit.ToString("X4");
    }

    static int Tables(string dir)
    {
        Assembly corlib = typeof(object).Assembly;
        using (Stream core = corlib.GetManifestResourceStream("collation.core.bin"))
        using (FileStream file = File.Create(Path.Combine(dir, "collation.core.bin")))
            core.CopyTo(file);

        var text = new StringBuilder();
        text.Append("# en-US as Mono ")
            .Append(MonoVersion())
            .Append(" collates it, written by dump_collation.cs.\n");
        text.Append("# lower <first> <last> <step> <delta>: TextInfo.ToLower adds delta to\n");
        text.Append("#   first, first + step, ... last; every other code unit maps to itself.\n");
        text.Append("# unassigned <first> <last>: char.GetUnicodeCategory is OtherNotAssigned.\n");
        text.Append("# contraction <unit> <replacement units>: SimpleCollator's contractions.\n");

        // Runs of the units ToLower moves, each one delta and one step apart.
        TextInfo info = EnUs.TextInfo;
        var moved = new List<int>();
        for (int unit = 0; unit <= 0xFFFF; unit++)
            if (info.ToLower((char)unit) != unit)
                moved.Add(unit);
        for (int at = 0; at < moved.Count; )
        {
            int first = moved[at];
            int delta = info.ToLower((char)first) - first;
            int step = 1,
                last = first,
                next = at + 1;
            if (
                next < moved.Count
                && info.ToLower((char)moved[next]) - moved[next] == delta
                && moved[next] - first <= 2
            )
            {
                step = moved[next] - first;
                while (
                    next < moved.Count
                    && moved[next] - last == step
                    && info.ToLower((char)moved[next]) - moved[next] == delta
                )
                    last = moved[next++];
            }
            text.Append("lower ")
                .Append(Hex(first))
                .Append(' ')
                .Append(Hex(last))
                .Append(' ')
                .Append(step)
                .Append(' ')
                .Append(delta)
                .Append('\n');
            at = next;
        }

        int gapStart = -1;
        for (int unit = 0; unit <= 0x10000; unit++)
        {
            bool unassigned =
                unit <= 0xFFFF
                && char.GetUnicodeCategory((char)unit) == UnicodeCategory.OtherNotAssigned;
            if (unassigned && gapStart < 0)
                gapStart = unit;
            if (!unassigned && gapStart >= 0)
            {
                text.Append("unassigned ")
                    .Append(Hex(gapStart))
                    .Append(' ')
                    .Append(Hex(unit - 1))
                    .Append('\n');
                gapStart = -1;
            }
        }

        Type collatorType = corlib.GetType("Mono.Globalization.Unicode.SimpleCollator");
        object collator = Activator.CreateInstance(
            collatorType,
            BindingFlags.Public | BindingFlags.Instance,
            null,
            new object[] { EnUs },
            null
        );
        var flags = BindingFlags.NonPublic | BindingFlags.Instance;
        if ((bool)collatorType.GetField("frenchSort", flags).GetValue(collator))
            throw new Exception("en-US sorts accents backwards; the port does not");
        if (((Array)collatorType.GetField("level2Maps", flags).GetValue(collator)).Length != 0)
            throw new Exception("en-US remaps diacritics; the port does not");
        foreach (
            object contraction in (Array)
                collatorType.GetField("contractions", flags).GetValue(collator)
        )
        {
            Type type = contraction.GetType();
            var source = (char[])type.GetField("Source").GetValue(contraction);
            var replacement = (string)type.GetField("Replacement").GetValue(contraction);
            if (source.Length != 1 || replacement == null)
                throw new Exception(
                    "a contraction of several units or to a sort key; the port has neither"
                );
            text.Append("contraction ").Append(Hex(source[0]));
            foreach (char unit in replacement)
                text.Append(' ').Append(Hex(unit));
            text.Append('\n');
        }
        File.WriteAllText(Path.Combine(dir, "culture-en-us.txt"), text.ToString());
        return 0;
    }

    static string MonoVersion()
    {
        Type type = Type.GetType("Mono.Runtime");
        MethodInfo display = type.GetMethod(
            "GetDisplayName",
            BindingFlags.NonPublic | BindingFlags.Static
        );
        return ((string)display.Invoke(null, null)).Split(' ')[0];
    }

    // The units the cases are made of: what the collator treats specially —
    // case, accents precomposed and combining, the expansions, ignorables,
    // kana with their extenders and special weights, surrogates whole and
    // halved — and plain letters for them to sit among.
    static readonly int[] Alphabet =
    {
        'a',
        'b',
        'c',
        'e',
        'f',
        'i',
        'l',
        's',
        't',
        'A',
        'B',
        'E',
        'F',
        'I',
        'S',
        '0',
        '1',
        ' ',
        '-',
        '\'',
        '.',
        0x09,
        0x00,
        0xAD,
        0x200B,
        0x200C,
        0x200D,
        0xFEFF,
        0xFDD0,
        0xFDEF,
        0xFFFF,
        0x0378,
        0xE9,
        0xC9,
        0xE8,
        0xFC,
        0xDC,
        0xF1,
        0xE5,
        0xC5,
        0xF8,
        0x301,
        0x308,
        0x327,
        0x30C,
        0x304,
        0xE6,
        0xC6,
        0x153,
        0x152,
        0xDF,
        0x1E9E,
        0xFB01,
        0xFB00,
        0xFB03,
        0x1C4,
        0x1C5,
        0x1C6,
        0x133,
        0x130,
        0x131,
        0x3A3,
        0x3C3,
        0x3C2,
        0x41F,
        0x43F,
        0x3042,
        0x30A2,
        0xFF71,
        0x3041,
        0x30FC,
        0x309D,
        0x309E,
        0x30FD,
        0x3005,
        0xFF70,
        0x3099,
        0xAC00,
        0x4E00,
        0x3400,
        0xFF21,
        0xFF41,
        0x3000,
        0x2126,
        0x212A,
        0xD83D,
        0xDE00,
        0xD801,
        0xDC00,
        0xE000,
        0x2190,
    };

    static string Encode(string text)
    {
        if (text.Length == 0)
            return "-";
        return string.Join(".", text.Select(unit => ((int)unit).ToString("X4")));
    }

    static string Answer(Func<int> search)
    {
        try
        {
            return search().ToString();
        }
        catch (ArgumentOutOfRangeException)
        {
            return "X";
        }
    }

    static string Flag(bool value)
    {
        return value ? "1" : "0";
    }

    static int Cases(string path, int seed, int count)
    {
        var random = new Random(seed);
        var lines = new StringBuilder();
        lines
            .Append("# Mono ")
            .Append(MonoVersion())
            .Append(", en-US, seed ")
            .Append(seed)
            .Append(", written by dump_collation.cs: one search per line,\n");
        lines.Append(
            "# source value start count last back, then for case-sensitive and then ignore-case:\n"
        );
        lines.Append("# IndexOf, LastIndexOf, StartsWith, EndsWith, IndexOf(start, count) and\n");
        lines.Append("# LastIndexOf(last, back); X an ArgumentOutOfRangeException.\n");
        CompareInfo compare = EnUs.CompareInfo;
        for (int n = 0; n < count; n++)
        {
            string source = Word(random, random.Next(0, 9));
            string value;
            int pick = random.Next(0, 4);
            if (pick < 2 && source.Length > 0)
            {
                int from = random.Next(0, source.Length);
                value = source.Substring(from, random.Next(1, source.Length - from + 1));
                if (pick == 1)
                    value = Mutate(random, value);
            }
            else
                value = Word(random, random.Next(1, 4));
            int start = random.Next(0, source.Length + 1);
            int span = random.Next(0, source.Length - start + 1);
            // LastIndexOf's start is where the search ends; its count runs
            // back from there, past what fits now and then.
            int last = random.Next(0, source.Length + 1);
            int back = random.Next(0, last + 4);
            lines
                .Append(Encode(source))
                .Append(' ')
                .Append(Encode(value))
                .Append(' ')
                .Append(start)
                .Append(' ')
                .Append(span)
                .Append(' ')
                .Append(last)
                .Append(' ')
                .Append(back);
            foreach (bool ignore in new[] { false, true })
            {
                var comparison = ignore
                    ? StringComparison.CurrentCultureIgnoreCase
                    : StringComparison.CurrentCulture;
                lines.Append(' ').Append(Answer(() => source.IndexOf(value, comparison)));
                lines.Append(' ').Append(Answer(() => source.LastIndexOf(value, comparison)));
                // String.StartsWith first answers true for the very same object;
                // the copy keeps every case on the collator's path.
                string copy = new string(value.ToCharArray());
                lines.Append(' ').Append(Flag(source.StartsWith(copy, ignore, EnUs)));
                lines.Append(' ').Append(Flag(source.EndsWith(copy, ignore, EnUs)));
                lines
                    .Append(' ')
                    .Append(Answer(() => source.IndexOf(value, start, span, comparison)));
                lines
                    .Append(' ')
                    .Append(Answer(() => source.LastIndexOf(value, last, back, comparison)));
            }
            lines.Append('\n');
        }
        File.WriteAllText(path, lines.ToString());
        return 0;
    }

    static string Word(Random random, int length)
    {
        var word = new StringBuilder();
        for (int n = 0; n < length; n++)
            word.Append((char)Alphabet[random.Next(0, Alphabet.Length)]);
        return word.ToString();
    }

    static string Mutate(Random random, string value)
    {
        var units = value.ToCharArray();
        int at = random.Next(0, units.Length);
        switch (random.Next(0, 3))
        {
            case 0:
                units[at] = char.IsUpper(units[at])
                    ? char.ToLowerInvariant(units[at])
                    : char.ToUpperInvariant(units[at]);
                return new string(units);
            case 1:
                return value.Insert(
                    at,
                    ((char)Alphabet[random.Next(0, Alphabet.Length)]).ToString()
                );
            default:
                units[at] = (char)Alphabet[random.Next(0, Alphabet.Length)];
                return new string(units);
        }
    }
}
