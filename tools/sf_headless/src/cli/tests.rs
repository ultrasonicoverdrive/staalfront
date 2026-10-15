use super::*;

fn args(list: &[&str]) -> Result<Parsed, String> {
    parse_args(list.iter().map(|s| s.to_string()))
}

fn run_args(list: &[&str]) -> Args {
    match args(list) {
        Ok(Parsed::Run(a)) => a,
        other => panic!("expected a run, got {other:?}"),
    }
}

fn error(list: &[&str]) -> String {
    args(list).unwrap_err()
}

#[test]
fn the_scenario_alone_gets_defaults() {
    let a = run_args(&["s.ron"]);
    assert_eq!(
        a,
        Args {
            scenario: "s.ron".into(),
            ticks: None,
            every: 100,
            out: None,
            expect: None,
        }
    );
}

#[test]
fn all_options() {
    let a = run_args(&[
        "s.ron", "--ticks", "500", "--every", "25", "--out", "log.json", "--expect", "0xAB",
    ]);
    assert_eq!(a.ticks, Some(500));
    assert_eq!(a.every, 25);
    assert_eq!(a.out, Some("log.json".into()));
    assert_eq!(a.expect, Some(0xAB));
}

#[test]
fn options_may_come_first_and_use_equals() {
    let a = run_args(&["--ticks=7", "--expect=ff", "s.ron", "--out=x/y.json"]);
    assert_eq!(a.scenario, PathBuf::from("s.ron"));
    assert_eq!((a.ticks, a.expect), (Some(7), Some(0xFF)));
    assert_eq!(a.out, Some("x/y.json".into()));
}

#[test]
fn help_wins() {
    assert_eq!(args(&["--help"]), Ok(Parsed::Help));
    assert_eq!(args(&["s.ron", "-h"]), Ok(Parsed::Help));
}

#[test]
fn usage_errors() {
    assert!(error(&[]).contains("no scenario"));
    assert!(error(&["a.ron", "b.ron"]).contains("more than one"));
    assert!(error(&["a.ron", "--bogus"]).contains("unknown option `--bogus`"));
    assert!(error(&["a.ron", "-x"]).contains("unknown option `-x`"));
    assert!(error(&["a.ron", "--ticks"]).contains("--ticks needs a value"));
    assert!(error(&["a.ron", "--ticks", "0"]).contains("at least 1"));
    assert!(error(&["a.ron", "--ticks", "ten"]).contains("whole number"));
    assert!(error(&["a.ron", "--ticks", "-5"]).contains("whole number"));
    assert!(error(&["a.ron", "--every", "0"]).contains("--every must be at least 1"));
    assert!(error(&["a.ron", "--ticks", "1", "--ticks", "2"]).contains("given twice"));
    assert!(error(&["a.ron", "--expect", "zz"]).contains("not a checksum"));
}

#[test]
fn a_value_is_taken_as_is_even_when_it_looks_like_a_flag() {
    assert_eq!(
        run_args(&["a.ron", "--out", "--ticks"]).out,
        Some("--ticks".into())
    );
}

#[test]
fn checksums_are_parsed_leniently() {
    assert_eq!(
        parse_checksum("0xf0bc_5e9a_6c67_35d9"),
        Ok(0xF0BC_5E9A_6C67_35D9)
    );
    assert_eq!(
        parse_checksum("F0BC5E9A6C6735D9"),
        Ok(0xF0BC_5E9A_6C67_35D9)
    );
    assert_eq!(parse_checksum("0X1"), Ok(1));
    // `$(cat file)` on Windows may end in a carriage return.
    assert_eq!(parse_checksum("0x00ab\r\n"), Ok(0xAB));
    assert_eq!(parse_checksum("  0x00ab "), Ok(0xAB));
}

#[test]
fn bad_checksums_are_refused() {
    for bad in [
        "",
        "0x",
        "0x1_0000_0000_0000_0000",
        "12345678901234567",
        "0xg1",
        "-1",
        "1.5",
    ] {
        assert!(parse_checksum(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn help_goes_to_stdout_and_exits_zero() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(run_cli(vec!["--help".into()], &mut out, &mut err), EXIT_OK);
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("Usage: sf_headless")
    );
    assert!(err.is_empty());
}

#[test]
fn usage_errors_exit_two_and_print_the_usage() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(run_cli(vec![], &mut out, &mut err), EXIT_INVALID);
    let err = String::from_utf8(err).unwrap();
    assert!(
        err.contains("no scenario file given") && err.contains("Usage:"),
        "{err}"
    );
    assert!(out.is_empty());
}
