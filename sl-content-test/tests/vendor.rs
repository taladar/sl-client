//! The committed vendor case, played end to end: it passes as written, and
//! fails — saying which script stopped, where and why, which expectations went
//! unmet and where the buyer got stuck — when one of its scripts is broken.

#[cfg(test)]
mod test {
    use std::path::{Path, PathBuf};

    use pretty_assertions::{assert_eq, assert_ne};
    use sl_content_test::{Case, check};

    type TestError = Box<dyn core::error::Error>;

    /// The vendor case's file.
    fn vendor_case() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("cases/vendor/case.toml")
    }

    /// The vendor's script, as committed.
    fn vendor_source() -> Result<String, TestError> {
        Ok(fs_err::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("cases/vendor/vendor.lsl"),
        )?)
    }

    /// **The case passes as written**: the notecard read, the menu answered,
    /// the child told, the payment taken and the gadget offered.
    #[tokio::test]
    async fn the_vendor_case_passes() -> Result<(), TestError> {
        let case = Case::load(&vendor_case())?;
        let verdicts = check(&case).await?;
        assert_eq!(verdicts.len(), 1, "one grid");
        for (verdict, observed) in &verdicts {
            assert!(
                verdict.passed(),
                "{verdict}\n{}",
                sl_content_test::verdict::transcript(observed)
            );
            assert!(verdict.notes.is_empty(), "{verdict}");
        }
        Ok(())
    }

    /// **A broken script fails the case informatively**: the vendor divides
    /// by a price it has zeroed, so its `money` handler stops with a Math
    /// Error. The report names the script, the line and the event; the thanks
    /// and the offer it never got to are unmet; and the buyer is reported
    /// stuck waiting for the thanks.
    #[tokio::test]
    async fn a_broken_vendor_fails_saying_where() -> Result<(), TestError> {
        let mut case = Case::load(&vendor_case())?;
        case.max_ticks = 200;
        let source = vendor_source()?;
        let broken = source.replace("amount / price", "amount / (price - price)");
        assert_ne!(broken, source, "the mutation found its line");
        assert!(case.replace_script("vendor", &broken));
        let line = broken
            .lines()
            .position(|text| text.contains("amount / (price - price)"))
            .ok_or("no broken line")?
            .saturating_add(1);

        let verdicts = check(&case).await?;
        let (verdict, _observed) = verdicts.first().ok_or("no verdict")?;
        assert!(!verdict.passed(), "{verdict}");
        let report = verdict.to_string();
        assert!(
            report.contains(&format!(
                "script \"vendor\" in \"Gadget Vendor\" stopped at line {line}, column"
            )) && report.contains("in money with Math Error"),
            "the fault names the script, its line and its event:\n{report}"
        );
        assert!(
            report.contains(
                "expected \"Gadget Vendor\" to say \"Thank you, Buyer Resident! Enjoy your Blue \
                 gadget.\" to Buyer Resident"
            ),
            "the thanks is unmet:\n{report}"
        );
        assert!(
            report.contains("expected Buyer Resident's transcript to hold, in order, line 4 of 4"),
            "the offer is unmet, the payment before it is not:\n{report}"
        );
        assert!(
            report.contains(
                "Buyer Resident had not finished its steps after 200 ticks; it was on step 6 of 6 \
                 (await_chat = \"Enjoy\")"
            ),
            "the buyer is stuck where it is:\n{report}"
        );
        Ok(())
    }

    /// **A script that does not compile is reported with its errors**, and
    /// fails the case on its own.
    #[tokio::test]
    async fn a_script_that_does_not_compile_is_reported() -> Result<(), TestError> {
        let mut case = Case::load(&vendor_case())?;
        case.max_ticks = 50;
        let broken =
            vendor_source()?.replace("integer gadgets = amount / price;", "integer gadgets = ;");
        assert!(case.replace_script("vendor", &broken));
        let verdicts = check(&case).await?;
        let (verdict, _observed) = verdicts.first().ok_or("no verdict")?;
        let report = verdict.to_string();
        assert!(
            report.contains("script \"vendor\" in \"Gadget Vendor\" does not compile:"),
            "{report}"
        );
        Ok(())
    }
}
