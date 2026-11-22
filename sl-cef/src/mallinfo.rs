//! A `mallinfo` that survives a heap over 2 GiB.
//!
//! Chromium's `MallocDumpProvider` reads glibc's legacy `mallinfo()`, whose
//! fields are `int`, and crashes on a failed check when `arena + hblkhd` comes
//! out negative. It expects its own allocator to hold nearly everything, so the
//! glibc heap to be small. Embedded in this viewer it is not: CEF's browser
//! process *is* the viewer's process, the viewer allocates through glibc, and the
//! first memory report after its heap passes 2 GiB killed the whole application
//! with `SIGILL`.
//!
//! The executable's definition of the symbol takes precedence over glibc's for
//! `libcef.so`, provided the executable exports it (the viewer's `build.rs`). It
//! reads the 64-bit `mallinfo2` and caps each field so that the sums Chromium
//! takes stay in range. These numbers feed only memory metrics.

use libc::c_int;

/// The largest value [`narrow`] reports for the two fields Chromium adds up.
const SUMMED_FIELD_LIMIT: c_int = c_int::MAX / 2;

/// `value` as a `c_int`, no larger than `limit`.
fn capped(value: usize, limit: c_int) -> c_int {
    c_int::try_from(value).unwrap_or(limit).min(limit)
}

/// The legacy report for a 64-bit one, every field capped rather than wrapped.
fn narrow(wide: &libc::mallinfo2) -> libc::mallinfo {
    libc::mallinfo {
        arena: capped(wide.arena, SUMMED_FIELD_LIMIT),
        ordblks: capped(wide.ordblks, c_int::MAX),
        smblks: capped(wide.smblks, c_int::MAX),
        hblks: capped(wide.hblks, c_int::MAX),
        hblkhd: capped(wide.hblkhd, SUMMED_FIELD_LIMIT),
        usmblks: capped(wide.usmblks, c_int::MAX),
        fsmblks: capped(wide.fsmblks, c_int::MAX),
        uordblks: capped(wide.uordblks, c_int::MAX),
        fordblks: capped(wide.fordblks, c_int::MAX),
        keepcost: capped(wide.keepcost, c_int::MAX),
    }
}

/// glibc's `mallinfo`, replaced for the whole process: see the
/// [module documentation](self).
#[unsafe(no_mangle)]
pub extern "C" fn mallinfo() -> libc::mallinfo {
    // SAFETY: `mallinfo2` takes no arguments and returns its report by value.
    let wide = unsafe { libc::mallinfo2() };
    narrow(&wide)
}

/// Keep [`mallinfo`] in the link: nothing in Rust calls it, and an object file
/// nothing refers to is left out of the executable.
pub(crate) fn retain() {
    let symbol: extern "C" fn() -> libc::mallinfo = mallinfo;
    let _retained = core::hint::black_box(symbol);
}

#[cfg(test)]
mod tests {
    use super::narrow;

    /// The heap of the crash: Chromium's sum must not go negative.
    #[test]
    fn a_heap_over_two_gib_keeps_chromiums_sum_in_range() {
        // SAFETY: `mallinfo2` is a plain struct of integers, valid when zeroed.
        let mut wide: libc::mallinfo2 = unsafe { core::mem::zeroed() };
        wide.arena = 0x9B5E_D0A0;
        wide.hblkhd = 0x9B5E_D0A0;
        wide.uordblks = 0x1_0000_0000;
        let report = narrow(&wide);
        assert!(report.arena.checked_add(report.hblkhd).is_some());
        assert!(report.arena > 0 && report.hblkhd > 0 && report.uordblks > 0);
    }

    /// A heap that fits is reported exactly.
    #[test]
    fn a_small_heap_is_reported_unchanged() {
        // SAFETY: as above.
        let mut wide: libc::mallinfo2 = unsafe { core::mem::zeroed() };
        wide.arena = 4096;
        wide.hblkhd = 8192;
        let report = narrow(&wide);
        assert!(report.arena == 4096 && report.hblkhd == 8192);
    }
}
