//! What running a script costs, in each engine's own measured time.
//!
//! The scheduler charges everything a script does in **nanoseconds of script
//! time**: what the operation took on the grid the region imitates. A budget
//! is a length of that time (a tick's share is the tick's length), so a
//! script runs as far in a tick as it would on the grid. The time is the
//! model's, never the machine's: what an operation is charged depends on the
//! operation and its operands alone, so how far a script gets is the same on
//! every run (book chapter `simulator/determinism.md`).
//!
//! The three engines differ by orders of magnitude, so each has its own
//! [`Speed`] ([`speed`]), fitted to its own transcripts of `vm/throughput`,
//! `vm/region-budget` and the `vm/call-cost-*` probes (book chapter
//! `gridspec/lsl.md`, § *What a call costs* and § *How many scripts a region
//! runs*):
//! Second Life's Mono runs an empty loop at 500 000 iterations a second and
//! pays some 16 µs an element to join a list, YEngine runs the loop at 28
//! million and a join costs it 7 µs and some 5 ns an element.

use crate::library::BuiltinId;
use crate::value::Value;
use crate::vm::flavour::EngineFlavour;
use sl_lsl::ast::BinaryOp;

/// A cost that grows with a size: measured points joined by straight lines,
/// flat before the first and following the last segment's slope after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeCost {
    /// `(size, nanoseconds)`, sizes strictly increasing and times never
    /// falling; at least two.
    points: &'static [(u64, u64)],
}

impl SizeCost {
    /// The time one operation of `size` takes, in nanoseconds.
    #[must_use]
    pub(crate) fn nanoseconds(&self, size: u64) -> u64 {
        let mut segments = self.points.windows(2);
        let segment = segments
            .find(|pair| matches!(pair, [_, (upper, _)] if size <= *upper))
            .or_else(|| self.points.windows(2).last());
        let Some(&[(x0, y0), (x1, y1)]) = segment else {
            return 0;
        };
        if size <= x0 {
            return y0;
        }
        // Straight between the two points, and on past the last one along the
        // same slope. A cost never falls as the size grows.
        let rise = y1.saturating_sub(y0);
        let run = x1.saturating_sub(x0).max(1);
        let along = size.saturating_sub(x0);
        y0.saturating_add(
            u64::try_from(
                u128::from(rise)
                    .saturating_mul(u128::from(along))
                    .checked_div(u128::from(run))
                    .unwrap_or(0),
            )
            .unwrap_or(u64::MAX),
        )
    }

    /// What one operation of `size` is charged: its time, as far as a charge
    /// holds it.
    #[must_use]
    pub(crate) fn charge(&self, size: u64) -> u32 {
        u32::try_from(self.nanoseconds(size)).unwrap_or(u32::MAX)
    }
}

/// One engine's speed: what each kind of operation is charged, in
/// nanoseconds, and how many scripts' shares its region runs in a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Speed {
    /// Every instruction. At least one: the scheduler takes a slice that
    /// charged nothing for one that had nothing to run.
    pub(crate) instruction: u32,
    /// A jump taken backwards — a loop going round again — on top of its
    /// instruction.
    pub(crate) backward_jump: u32,
    /// Calling a library function, on top of the `CallBuiltin` instruction.
    pub(crate) builtin_call: u32,
    /// How many scripts' full shares the region runs per tick before the
    /// rest wait for the next: the region-wide budget, as a multiple of the
    /// per-script one.
    pub(crate) region_shares: u64,
    /// `list + list` (and a list joined with one value), by the length of the
    /// list it makes; measured as `r = L + [-1]`.
    list_join: SizeCost,
    /// `string + string`, by the characters of the string it makes; measured
    /// as `s = S + "x"`.
    string_join: SizeCost,
    /// The library functions whose work grows with an argument.
    library: &'static Curves,
}

/// The measured size costs of the string and list functions, each the time
/// one call takes above an empty loop — the flat call included — at the
/// probes' sizes. [`None`] where the engine's cost does not grow, or is lost
/// in the noise of the empty loop's: the flat call alone.
///
/// A function the probes did not time is charged as the timed one that does
/// the same work (`library_cost`).
#[derive(Debug, PartialEq, Eq)]
struct Curves {
    /// `llList2Integer`.
    list_read: Option<SizeCost>,
    /// `llList2List(L, 0, -1)`.
    list_copy: Option<SizeCost>,
    /// `llListReplaceList`, replacing the first element.
    list_replace: Option<SizeCost>,
    /// `llListSort`, ascending, stride 1.
    list_sort: Option<SizeCost>,
    /// `llListFindList` of a one-element pattern that is not there.
    list_find: Option<SizeCost>,
    /// `llDeleteSubList` of the first element.
    delete_sub_list: Option<SizeCost>,
    /// `llList2CSV`.
    list_to_csv: Option<SizeCost>,
    /// `llDumpList2String`.
    dump_list: Option<SizeCost>,
    /// `llListStatistics(LIST_STAT_SUM, …)`.
    list_statistics: Option<SizeCost>,
    /// `llListRandomize`, stride 1.
    list_randomize: Option<SizeCost>,
    /// `llCSV2List`, by the elements of the list the text was made from.
    csv_to_list: Option<SizeCost>,
    /// `llStringLength`.
    string_length: Option<SizeCost>,
    /// `llGetSubString` of the last character.
    get_sub_string: Option<SizeCost>,
    /// `llSubStringIndex` of a character that is not there.
    sub_string_index: Option<SizeCost>,
    /// `llParseString2List` on one separator.
    parse: Option<SizeCost>,
    /// `llToUpper`.
    upper: Option<SizeCost>,
    /// `llStringToBase64`.
    to_base64: Option<SizeCost>,
    /// `llMD5String`.
    md5: Option<SizeCost>,
    /// `llSHA1String`.
    sha1: Option<SizeCost>,
    /// `llSHA256String`.
    sha256: Option<SizeCost>,
    /// `llComputeHash(…, "sha256")`.
    compute_hash: Option<SizeCost>,
    /// `llEscapeURL`.
    escape: Option<SizeCost>,
    /// `llInsertString` at the start.
    insert_string: Option<SizeCost>,
    /// `llDeleteSubString` of the first character.
    delete_sub_string: Option<SizeCost>,
    /// `llStringTrim(…, STRING_TRIM)`.
    string_trim: Option<SizeCost>,
    /// `llReplaceSubString` of every comma.
    replace_sub_string: Option<SizeCost>,
    /// `llOrd` of the last character.
    ord: Option<SizeCost>,
    /// `llHash`.
    hash: Option<SizeCost>,
}

/// The speed of the engine `flavour` imitates.
#[must_use]
pub(crate) const fn speed(flavour: EngineFlavour) -> &'static Speed {
    match flavour {
        EngineFlavour::SecondLife => &SECOND_LIFE,
        EngineFlavour::YEngine => &YENGINE,
        EngineFlavour::XEngine => &XENGINE,
    }
}

/// Second Life's Mono.
///
/// **The loop.** Measured on aditi (Mono, 2026-09-28, one busy script in a
/// quiet region, loops of 400 000 and 1 000 000 iterations, about two seconds
/// each, so `llGetTime`'s whole-frame steps are a 1 % error): an empty `for`
/// loop, eight of our instructions an iteration, runs at 500 000 iterations a
/// second (four runs, 499 806 to 500 141), 2 µs an iteration, and one that
/// also assigns, multiplies and adds (fourteen) at 494 300 — six more
/// instructions cost barely one per cent. Mono's time is in the scheduler
/// check at each loop back-edge, not in the arithmetic between them: 4 ns an
/// instruction and 1 968 a back-edge predict both loops, 2 000 and 2 024 ns.
///
/// **A call.** A loop calling `llAbs` runs at 226 400 iterations a second,
/// 4 417 ns: eleven instructions and a back-edge (2 012) and 2 405 for the
/// call.
///
/// **The region.** Measured on aditi (Mauve, 2026-10-29, probe
/// `vm/region-budget`): 1, 16, 64 and 128 busy scripts, each an empty loop of
/// its own, every one of them keeps the rate one runs at alone (469 680,
/// 476 508, 478 536 and 477 162 iterations a second); 256 run at half that
/// (241 793), the region's total staying at 61.9 million — 130 one-script
/// shares. An earlier run the same evening read 134. The region's script time
/// rose with the scripts to 23.9 ms, a whole frame at 45 frames a second, and
/// `llGetTime` kept pace with real time throughout: a script sees its rate
/// fall, not its clock slow.
///
/// **Size costs.** The aditi transcripts of `vm/call-cost-lists`,
/// `vm/call-cost-strings` and `vm/call-cost-newer` (Mono, 2026-10-27). A new
/// list costs Mono as much as 75 library calls before it holds anything, and
/// then about 16 µs an element: building a list with `+=` in a loop is
/// quadratic, and as slow as on the grid. The two hours-apart aditi runs
/// differed by up to a sixth at the small sizes (book: 161–186 µs at one
/// element); these are the committed run's. A string join is four library
/// calls before the first character, then about 0.19 µs one.
const SECOND_LIFE: Speed = Speed {
    instruction: 4,
    backward_jump: 1_968,
    builtin_call: 2_405,
    region_shares: 130,
    list_join: SizeCost {
        points: &[
            (2, 185_618),
            (11, 197_516),
            (101, 1_334_644),
            (501, 7_775_998),
        ],
    },
    string_join: SizeCost {
        points: &[(2, 10_753), (11, 11_290), (101, 38_172), (1001, 209_511)],
    },
    library: &SECOND_LIFE_CURVES,
};

/// Second Life's size costs. Where the committed run has an outlier it is
/// left out: `llList2Integer`'s 7.8 µs at one element (the other run's 4.2),
/// `llList2List`'s 113 µs at ten (below its own 183 at one) and
/// `llListReplaceList`'s 661 µs at one (book: 63–661), so a short list costs
/// what ten elements do. Where a one-character run is a hair above the ten's
/// (`llComputeHash`, `llSHA256String`) it is taken as the ten's.
const SECOND_LIFE_CURVES: Curves = Curves {
    list_read: Some(SizeCost {
        points: &[(10, 4_186), (500, 4_347)],
    }),
    list_copy: Some(SizeCost {
        points: &[(1, 182_938), (100, 973_828), (500, 6_540_191)],
    }),
    list_replace: Some(SizeCost {
        points: &[(10, 132_073), (100, 1_676_489), (500, 9_149_324)],
    }),
    list_sort: Some(SizeCost {
        points: &[
            (1, 197_946),
            (10, 334_354),
            (100, 5_285_062),
            (500, 27_417_565),
        ],
    }),
    list_find: Some(SizeCost {
        points: &[(1, 49_991), (10, 83_139), (100, 440_175), (500, 2_483_298)],
    }),
    delete_sub_list: Some(SizeCost {
        points: &[
            (1, 86_285),
            (10, 210_151),
            (100, 1_886_019),
            (500, 7_167_584),
        ],
    }),
    list_to_csv: Some(SizeCost {
        points: &[
            (1, 99_417),
            (10, 313_276),
            (100, 2_471_874),
            (500, 12_777_573),
        ],
    }),
    dump_list: Some(SizeCost {
        points: &[
            (1, 102_274),
            (10, 362_066),
            (100, 2_904_112),
            (500, 27_114_692),
        ],
    }),
    list_statistics: Some(SizeCost {
        points: &[(1, 28_944), (10, 55_922), (100, 513_168), (500, 2_796_468)],
    }),
    list_randomize: Some(SizeCost {
        points: &[
            (1, 169_445),
            (10, 365_696),
            (100, 3_354_688),
            (500, 18_887_635),
        ],
    }),
    csv_to_list: Some(SizeCost {
        points: &[
            (1, 166_645),
            (10, 405_534),
            (100, 4_379_347),
            (500, 18_885_685),
        ],
    }),
    string_length: Some(SizeCost {
        points: &[(1, 15_422), (10, 30_288), (100, 98_368), (1000, 670_717)],
    }),
    get_sub_string: Some(SizeCost {
        points: &[(1, 36_055), (10, 48_745), (100, 120_761), (1000, 751_811)],
    }),
    sub_string_index: Some(SizeCost {
        points: &[(1, 25_671), (10, 41_002), (100, 116_029), (1000, 766_834)],
    }),
    parse: Some(SizeCost {
        points: &[
            (1, 147_767),
            (10, 453_595),
            (100, 4_933_342),
            (1000, 19_161_885),
        ],
    }),
    upper: Some(SizeCost {
        points: &[(1, 14_117), (10, 19_378), (100, 86_017), (1000, 504_918)],
    }),
    to_base64: Some(SizeCost {
        points: &[(1, 33_517), (10, 50_932), (100, 275_948), (1000, 2_357_771)],
    }),
    md5: Some(SizeCost {
        points: &[(1, 240_023), (10, 240_569), (100, 326_253), (1000, 964_317)],
    }),
    sha1: Some(SizeCost {
        points: &[(1, 268_514), (10, 307_447), (100, 346_942), (1000, 845_389)],
    }),
    sha256: Some(SizeCost {
        points: &[(1, 381_674), (10, 381_674), (100, 433_737), (1000, 953_666)],
    }),
    compute_hash: Some(SizeCost {
        points: &[
            (1, 195_911),
            (10, 195_911),
            (100, 274_394),
            (1000, 1_150_634),
        ],
    }),
    escape: Some(SizeCost {
        points: &[
            (1, 70_524),
            (10, 146_215),
            (100, 1_044_984),
            (1000, 7_190_857),
        ],
    }),
    insert_string: Some(SizeCost {
        points: &[(1, 52_990), (10, 99_066), (100, 466_395), (1000, 3_774_445)],
    }),
    delete_sub_string: Some(SizeCost {
        points: &[(1, 32_434), (10, 81_219), (100, 453_622), (1000, 3_972_495)],
    }),
    string_trim: Some(SizeCost {
        points: &[(1, 38_270), (10, 77_035), (100, 466_915), (1000, 3_927_881)],
    }),
    replace_sub_string: Some(SizeCost {
        points: &[
            (1, 59_424),
            (10, 122_274),
            (100, 576_031),
            (1000, 4_644_208),
        ],
    }),
    ord: Some(SizeCost {
        points: &[(1, 15_940), (10, 32_809), (100, 109_901), (1000, 741_545)],
    }),
    hash: Some(SizeCost {
        points: &[(1, 15_371), (10, 32_734), (100, 119_884), (1000, 907_197)],
    }),
};

/// OpenSim's YEngine, which compiles a script to CIL the runtime JITs.
///
/// **The loop.** Measured on the local OpenSim (0.9.2.1, 2026-10-23 and -29):
/// an empty loop runs at 27.8 and 30.3 million iterations a second
/// (`vm/throughput`, a million iterations in 33 to 36 ms, so a millisecond's
/// clock step is three per cent) and 28.5 million over the longer
/// `vm/region-budget` stage, 35 ns an iteration; three more operations in the
/// body run at 30.3 and 29.4 million, no slower. The arithmetic costs nothing
/// a clock can see; the time is the back-edge's, where YEngine checks whether
/// the script should yield. An instruction is still charged a nanosecond, the
/// least a charge can be — the scheduler takes a slice that charged nothing
/// for one that had nothing to run — so the back-edge is 27 and the loop 35;
/// the three-operation loop is charged 41 ns, a sixth slower than measured.
///
/// **A call.** `llGetListLength` and `llStringLength` cost 3 to 4 ns above
/// the loop (`vm/call-cost-*`), which is the flat call. `llAbs` costs 52 in
/// `vm/throughput` (87 ns an iteration) and is not modelled apart.
///
/// **The region.** YEngine runs scripts on `NumThreadScriptWorkers` threads,
/// two by default (`XMREngine.cs`), and the region's total under 16 busy
/// scripts was 62.9 million iterations a second, 2.2 one-script shares; it
/// rose to 3.8 under 256 as the slices' own overhead fell away. Two.
///
/// **Size costs.** The local OpenSim's `vm/call-cost-lists` and
/// `vm/call-cost-strings` (2026-10-23). Every call that makes a new list pays
/// some 7 µs before it holds anything and then about 5 ns an element.
const YENGINE: Speed = Speed {
    instruction: 1,
    backward_jump: 27,
    builtin_call: 4,
    region_shares: 2,
    list_join: SizeCost {
        points: &[(2, 6_956), (11, 7_013), (101, 7_646), (501, 9_577)],
    },
    string_join: SizeCost {
        points: &[(2, 71), (11, 77), (101, 159), (1001, 942)],
    },
    library: &YENGINE_CURVES,
};

/// YEngine's size costs. `llStringLength` is the flat call at every size;
/// `llReplaceSubString`, `llSHA256String`, `llComputeHash`, `llOrd` and
/// `llHash` were not timed, the probe stopping at OpenSim's compile error for
/// the first three.
const YENGINE_CURVES: Curves = Curves {
    list_read: Some(SizeCost {
        points: &[(1, 12), (500, 12)],
    }),
    list_copy: Some(SizeCost {
        points: &[(1, 6_980), (10, 7_045), (100, 7_664), (500, 9_668)],
    }),
    list_replace: Some(SizeCost {
        points: &[(1, 6_810), (10, 7_197), (100, 8_242), (500, 11_588)],
    }),
    list_sort: Some(SizeCost {
        points: &[(1, 7_091), (10, 7_410), (100, 15_759), (500, 68_511)],
    }),
    list_find: Some(SizeCost {
        points: &[(1, 53), (10, 386), (100, 3_630), (500, 18_019)],
    }),
    delete_sub_list: Some(SizeCost {
        points: &[(1, 6_860), (10, 6_991), (100, 7_730), (500, 9_718)],
    }),
    list_to_csv: Some(SizeCost {
        points: &[(1, 363), (10, 1_451), (100, 11_640), (500, 56_198)],
    }),
    dump_list: Some(SizeCost {
        points: &[(1, 207), (10, 1_783), (100, 43_199), (500, 683_591)],
    }),
    list_statistics: Some(SizeCost {
        points: &[(1, 253), (10, 2_392), (100, 23_279), (500, 114_284)],
    }),
    list_randomize: Some(SizeCost {
        points: &[(1, 8_104), (10, 10_844), (100, 38_109), (500, 497_306)],
    }),
    csv_to_list: Some(SizeCost {
        points: &[(1, 6_923), (10, 9_430), (100, 48_373), (500, 622_362)],
    }),
    string_length: None,
    get_sub_string: Some(SizeCost {
        points: &[(1, 27), (10, 61), (100, 61), (1000, 63)],
    }),
    sub_string_index: Some(SizeCost {
        points: &[(1, 66), (10, 119), (100, 750), (1000, 7_057)],
    }),
    parse: Some(SizeCost {
        points: &[(1, 7_398), (10, 8_081), (100, 13_631), (1000, 96_292)],
    }),
    upper: Some(SizeCost {
        points: &[(1, 103), (10, 184), (100, 913), (1000, 8_260)],
    }),
    to_base64: Some(SizeCost {
        points: &[(1, 159), (10, 222), (100, 714), (1000, 5_012)],
    }),
    md5: Some(SizeCost {
        points: &[(1, 6_967), (10, 7_392), (100, 7_577), (1000, 14_336)],
    }),
    sha1: Some(SizeCost {
        points: &[(1, 1_969), (10, 2_080), (100, 2_532), (1000, 8_474)],
    }),
    sha256: None,
    compute_hash: None,
    escape: Some(SizeCost {
        points: &[(1, 55), (10, 421), (100, 2_241), (1000, 29_527)],
    }),
    insert_string: Some(SizeCost {
        points: &[(1, 102), (10, 110), (100, 186), (1000, 956)],
    }),
    delete_sub_string: Some(SizeCost {
        points: &[(1, 19), (10, 84), (100, 163), (1000, 934)],
    }),
    string_trim: Some(SizeCost {
        points: &[(1, 72), (1000, 73)],
    }),
    replace_sub_string: None,
    ord: None,
    hash: None,
};

/// OpenSim's XEngine, which compiles a script to C# and runs it on a thread
/// pool.
///
/// **The loop.** Measured on the local OpenSim (0.9.2.1, 2026-10-23 and -29):
/// an empty loop runs at 437 600 and 416 000 iterations a second
/// (`vm/throughput`) and 436 880 over the `vm/region-budget` stage, 2 289 ns
/// an iteration; three more operations run at 439 900 and 438 000, no slower.
/// As under YEngine the time is the back-edge's: a nanosecond an
/// instruction, the least a charge can be, and 2 281 the back-edge.
///
/// **A call.** A loop calling `llAbs` runs at 432 300 and 433 700, 21 ns more
/// than the empty loop's 2 289 — the flat call. The `vm/call-cost-*` cells of
/// the functions whose cost does not grow are lost in the empty loop's own
/// noise, some 150 ns either way.
///
/// **The region.** XEngine runs scripts on a pool of up to `MaxThreads`
/// threads, 100 by default (`XEngine.cs`, `OpenSimDefaults.ini`): the
/// region's total rose with the scripts, to 13.6 one-script shares under 16,
/// 49 under 64 and 83 under 128 (the run never reported 256). A hundred.
///
/// **Size costs.** The local OpenSim's `vm/call-cost-lists` and
/// `vm/call-cost-strings` (2026-10-23). A list join costs 175 ns and some
/// 4 ns an element.
const XENGINE: Speed = Speed {
    instruction: 1,
    backward_jump: 2_281,
    builtin_call: 21,
    region_shares: 100,
    list_join: SizeCost {
        points: &[(2, 175), (11, 253), (101, 795), (501, 2_224)],
    },
    string_join: SizeCost {
        points: &[(2, 144), (11, 182), (101, 335), (1001, 1_317)],
    },
    library: &XENGINE_CURVES,
};

/// XEngine's size costs. `llList2Integer`, `llStringLength`, `llGetSubString`
/// and `llStringTrim` are the flat call within the noise at every size, and
/// `llListReplaceList`'s one-element cell (-1 ns) is taken as nothing; the
/// functions YEngine's run did not time, XEngine's did not either.
const XENGINE_CURVES: Curves = Curves {
    list_read: None,
    list_copy: Some(SizeCost {
        points: &[(1, 153), (10, 257), (100, 822), (500, 2_588)],
    }),
    list_replace: Some(SizeCost {
        points: &[(1, 0), (10, 368), (100, 1_319), (500, 4_609)],
    }),
    list_sort: Some(SizeCost {
        points: &[(1, 105), (10, 778), (100, 9_625), (500, 66_486)],
    }),
    list_find: Some(SizeCost {
        points: &[(1, 184), (10, 699), (100, 4_594), (500, 22_362)],
    }),
    delete_sub_list: Some(SizeCost {
        points: &[(1, 29), (10, 248), (100, 855), (500, 2_212)],
    }),
    list_to_csv: Some(SizeCost {
        points: &[(1, 611), (10, 1_867), (100, 13_500), (500, 64_476)],
    }),
    dump_list: Some(SizeCost {
        points: &[(1, 522), (10, 2_513), (100, 46_750), (500, 902_181)],
    }),
    list_statistics: Some(SizeCost {
        points: &[(1, 455), (10, 2_960), (100, 25_054), (500, 133_108)],
    }),
    list_randomize: Some(SizeCost {
        points: &[(1, 1_516), (10, 4_264), (100, 33_890), (500, 581_798)],
    }),
    csv_to_list: Some(SizeCost {
        points: &[(1, 238), (10, 2_666), (100, 47_094), (500, 626_232)],
    }),
    string_length: None,
    get_sub_string: None,
    sub_string_index: Some(SizeCost {
        points: &[(1, 9), (10, 355), (100, 959), (1000, 7_716)],
    }),
    parse: Some(SizeCost {
        points: &[(1, 528), (10, 1_261), (100, 7_195), (1000, 96_101)],
    }),
    upper: Some(SizeCost {
        points: &[(1, 10), (10, 244), (100, 1_054), (1000, 9_618)],
    }),
    to_base64: Some(SizeCost {
        points: &[(1, 192), (10, 317), (100, 820), (1000, 5_684)],
    }),
    md5: Some(SizeCost {
        points: &[(1, 7_604), (10, 8_230), (100, 8_374), (1000, 16_669)],
    }),
    sha1: Some(SizeCost {
        points: &[(1, 2_348), (10, 2_612), (100, 3_034), (1000, 9_808)],
    }),
    sha256: None,
    compute_hash: None,
    escape: Some(SizeCost {
        points: &[(1, 33), (10, 555), (100, 2_463), (1000, 32_309)],
    }),
    insert_string: Some(SizeCost {
        points: &[(1, 38), (10, 261), (100, 415), (1000, 1_259)],
    }),
    delete_sub_string: Some(SizeCost {
        points: &[(1, 72), (10, 167), (100, 227), (1000, 1_289)],
    }),
    string_trim: None,
    replace_sub_string: None,
    ord: None,
    hash: None,
};

impl Speed {
    /// What the operator `op` costs on top of its own instruction, given the
    /// `value` it produced: a join of lists or strings grows with the result;
    /// any other operator is the one instruction.
    #[must_use]
    pub(crate) fn operator(&self, op: BinaryOp, value: &Value) -> u32 {
        if !matches!(op, BinaryOp::Add) {
            return 0;
        }
        match value {
            Value::List(elements) => self.list_join.charge(size(elements.len())),
            Value::String(text) => self.string_join.charge(size(text.chars().count())),
            Value::Integer(_)
            | Value::Float(_)
            | Value::Key(_)
            | Value::Vector(_)
            | Value::Rotation(_) => 0,
        }
    }

    /// What calling the library function `id` with `args` costs on top of
    /// [`Self::builtin_call`]: the time its arguments' size costs on the grid
    /// less the flat call's (see `library_cost`), or nothing for a function
    /// whose work does not grow.
    #[must_use]
    pub(crate) fn call(&self, id: BuiltinId, args: &[Value]) -> u32 {
        library_cost(self.library, id).map_or(0, |(cost, measure)| {
            cost.charge(measure.of(args))
                .saturating_sub(self.builtin_call)
        })
    }

    /// The list join's curve, for the tests that pay one off.
    #[cfg(test)]
    pub(crate) const fn list_join(&self) -> SizeCost {
        self.list_join
    }
}

/// What a library call's cost grows with: the length of one of its
/// arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    /// The elements of the list at this argument.
    List(usize),
    /// The characters of the string at this argument.
    Text(usize),
    /// The comma-separated elements of the string at this argument (the
    /// probe timed `llCSV2List` by the elements of the list it was made
    /// from).
    CsvElements(usize),
}

impl Measure {
    /// The size of `args` by this measure; 0 when the argument is not there
    /// or is of another type.
    fn of(self, args: &[Value]) -> u64 {
        match self {
            Self::List(at) => match args.get(at) {
                Some(Value::List(elements)) => size(elements.len()),
                _ => 0,
            },
            Self::Text(at) => match args.get(at) {
                Some(Value::String(text) | Value::Key(text)) => size(text.chars().count()),
                _ => 0,
            },
            Self::CsvElements(at) => match args.get(at) {
                Some(Value::String(text)) => size(text.matches(',').count().saturating_add(1)),
                _ => 0,
            },
        }
    }
}

/// The size cost of the library function `id` under `curves`, with what it
/// grows with; [`None`] for a function that costs the flat call alone.
///
/// A function the probes did not time is charged as the timed one that does
/// the same work: `llToLower` as `llToUpper`, `llUnescapeURL` as
/// `llEscapeURL`, `llBase64ToString` and the XOR functions as
/// `llStringToBase64`, `llParseStringKeepNulls` as `llParseString2List`,
/// every other single-element read as `llList2Integer`, `llListInsertList`
/// as `llListReplaceList`, the strided and sliced copies as `llList2List`,
/// `llListSortStrided` as `llListSort`, the other searches as
/// `llListFindList`, and `llHMAC` as `llComputeHash`.
fn library_cost(curves: &Curves, id: BuiltinId) -> Option<(SizeCost, Measure)> {
    use BuiltinId as F;
    use Measure::{CsvElements, List, Text};
    let (curve, measure) = match id {
        F::LlList2Integer
        | F::LlList2String
        | F::LlList2Float
        | F::LlList2Key
        | F::LlList2Vector
        | F::LlList2Rot
        | F::LlGetListEntryType => (curves.list_read, List(0)),
        F::LlList2List | F::LlList2ListStrided | F::LlList2ListSlice => (curves.list_copy, List(0)),
        F::LlListReplaceList | F::LlListInsertList => (curves.list_replace, List(0)),
        F::LlListSort | F::LlListSortStrided => (curves.list_sort, List(0)),
        F::LlListFindList | F::LlListFindListNext | F::LlListFindStrided => {
            (curves.list_find, List(0))
        }
        F::LlDeleteSubList => (curves.delete_sub_list, List(0)),
        F::LlList2CSV => (curves.list_to_csv, List(0)),
        F::LlDumpList2String => (curves.dump_list, List(0)),
        F::LlListStatistics => (curves.list_statistics, List(1)),
        F::LlListRandomize => (curves.list_randomize, List(0)),
        F::LlCSV2List => (curves.csv_to_list, CsvElements(0)),
        F::LlStringLength => (curves.string_length, Text(0)),
        F::LlGetSubString => (curves.get_sub_string, Text(0)),
        F::LlSubStringIndex => (curves.sub_string_index, Text(0)),
        F::LlParseString2List | F::LlParseStringKeepNulls => (curves.parse, Text(0)),
        F::LlToUpper | F::LlToLower => (curves.upper, Text(0)),
        F::LlStringToBase64
        | F::LlBase64ToString
        | F::LlXorBase64
        | F::LlXorBase64Strings
        | F::LlXorBase64StringsCorrect => (curves.to_base64, Text(0)),
        F::LlMD5String => (curves.md5, Text(0)),
        F::LlSHA1String => (curves.sha1, Text(0)),
        F::LlSHA256String => (curves.sha256, Text(0)),
        F::LlComputeHash => (curves.compute_hash, Text(0)),
        F::LlHMAC => (curves.compute_hash, Text(1)),
        F::LlEscapeURL | F::LlUnescapeURL => (curves.escape, Text(0)),
        F::LlInsertString => (curves.insert_string, Text(0)),
        F::LlDeleteSubString => (curves.delete_sub_string, Text(0)),
        F::LlStringTrim => (curves.string_trim, Text(0)),
        F::LlReplaceSubString => (curves.replace_sub_string, Text(0)),
        F::LlOrd => (curves.ord, Text(0)),
        F::LlHash => (curves.hash, Text(0)),
        _ => return None,
    };
    Some((curve?, measure))
}

/// A length as a size.
fn size(length: usize) -> u64 {
    u64::try_from(length).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{SizeCost, Speed, library_cost, speed};
    use crate::vm::flavour::EngineFlavour;

    /// Every flavour's speed.
    const ALL: [EngineFlavour; 3] = [
        EngineFlavour::SecondLife,
        EngineFlavour::YEngine,
        EngineFlavour::XEngine,
    ];

    /// The charges of `cost` at every size up to 2 000, never falling.
    fn never_falls(cost: SizeCost) -> bool {
        let charges: Vec<u32> = (0..2000).map(|size| cost.charge(size)).collect();
        charges
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a <= b))
    }

    #[test]
    fn a_size_cost_passes_through_its_points_and_runs_on_past_the_last() {
        let cost = SizeCost {
            points: &[(2, 100), (12, 200), (22, 400)],
        };
        assert_eq!(cost.nanoseconds(0), 100);
        assert_eq!(cost.nanoseconds(2), 100);
        assert_eq!(cost.nanoseconds(7), 150);
        assert_eq!(cost.nanoseconds(12), 200);
        assert_eq!(cost.nanoseconds(17), 300);
        assert_eq!(cost.nanoseconds(22), 400);
        assert_eq!(cost.nanoseconds(32), 600);
    }

    #[test]
    fn the_joins_never_get_cheaper_as_they_grow() {
        for flavour in ALL {
            let Speed {
                list_join,
                string_join,
                ..
            } = *speed(flavour);
            assert!(never_falls(list_join), "{flavour:?} list");
            assert!(never_falls(string_join), "{flavour:?} string");
        }
    }

    #[test]
    fn no_library_call_gets_cheaper_as_its_argument_grows() {
        for flavour in ALL {
            for id in crate::library::BuiltinId::ALL {
                let Some((cost, _)) = library_cost(speed(flavour).library, id) else {
                    continue;
                };
                assert!(never_falls(cost), "{flavour:?} {id:?}");
            }
        }
    }

    #[test]
    fn every_instruction_is_charged_something() {
        for flavour in ALL {
            assert!(speed(flavour).instruction > 0, "{flavour:?}");
        }
    }

    #[test]
    fn an_empty_loop_runs_at_each_grids_rate() {
        // Eight instructions an iteration, one of them the back-edge.
        for (flavour, per_second) in [
            (EngineFlavour::SecondLife, 500_000),
            (EngineFlavour::YEngine, 28_571_428),
            (EngineFlavour::XEngine, 436_871),
        ] {
            let speed = speed(flavour);
            let iteration = u64::from(
                speed
                    .instruction
                    .saturating_mul(8)
                    .saturating_add(speed.backward_jump),
            );
            assert_eq!(1_000_000_000 / iteration, per_second, "{flavour:?}");
        }
    }
}
