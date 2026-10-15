use super::*;
use crate::run::Sample;

fn outcome() -> Outcome {
    Outcome {
        scenario: "demo".into(),
        seed: 0x57A1_F407,
        map_width: 64,
        map_height: 32,
        ticks: 20,
        every: 10,
        commands_scheduled: 5,
        commands_executed: 4,
        commands_ignored: 1,
        entities: 3,
        initial_checksum: 0xAB,
        final_checksum: 0xFFFF_FFFF_FFFF_FFFF,
        samples: vec![
            Sample {
                ticks: 10,
                checksum: 0x1234,
            },
            Sample {
                ticks: 20,
                checksum: 0xFFFF_FFFF_FFFF_FFFF,
            },
        ],
    }
}

fn host() -> Host {
    Host {
        os: "linux",
        arch: "x86_64",
        debug_assertions: false,
        elapsed_micros: 2_000_000,
    }
}

const EXPECTED: &str = r#"{
  "format": 1,
  "scenario": "demo",
  "seed": "0x0000000057a1f407",
  "map": {"width": 64, "height": 32},
  "ticks": 20,
  "every": 10,
  "commands_scheduled": 5,
  "commands_executed": 4,
  "commands_ignored": 1,
  "entities": 3,
  "initial_checksum": "0x00000000000000ab",
  "final_checksum": "0xffffffffffffffff",
  "checksums": [
    {"ticks": 10, "checksum": "0x0000000000001234"},
    {"ticks": 20, "checksum": "0xffffffffffffffff"}
  ],
  "host": {
    "os": "linux",
    "arch": "x86_64",
    "debug_assertions": false,
    "elapsed_ms": 2000,
    "ticks_per_second": 10
  }
}
"#;

#[test]
fn the_layout_is_fixed() {
    assert_eq!(render(&outcome(), &host()), EXPECTED);
}

#[test]
fn the_host_block_is_last_and_the_only_part_that_depends_on_the_machine() {
    let a = render(&outcome(), &host());
    let other = Host {
        os: "windows",
        arch: "aarch64",
        debug_assertions: true,
        elapsed_micros: 77,
    };
    let b = render(&outcome(), &other);
    assert_ne!(a, b);
    let head = |s: &str| s[..s.find("  \"host\": {").unwrap()].to_string();
    assert_eq!(head(&a), head(&b));
    // Nothing follows the host block but the closing braces.
    assert!(a.ends_with("  }\n}\n"));
    assert_eq!(a.matches("\"host\"").count(), 1);
}

#[test]
fn lines_end_in_a_bare_newline() {
    assert!(!render(&outcome(), &host()).contains('\r'));
}

#[test]
fn a_single_checksum_has_no_trailing_comma() {
    let mut o = outcome();
    o.samples.truncate(1);
    let text = render(&o, &host());
    assert!(text.contains("\"checksum\": \"0x0000000000001234\"}\n  ],"));
}

#[test]
fn hex_is_sixteen_lowercase_digits() {
    assert_eq!(hex(0), "0x0000000000000000");
    assert_eq!(hex(0xABCDEF), "0x0000000000abcdef");
    assert_eq!(hex(u64::MAX), "0xffffffffffffffff");
}

#[test]
fn strings_are_escaped() {
    assert_eq!(json_string("plain"), "\"plain\"");
    assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
    assert_eq!(json_string("line\nnext\ttab\r"), r#""line\nnext\ttab\r""#);
    assert_eq!(json_string("\u{1}\u{1f}"), r#""\u0001\u001f""#);
    assert_eq!(json_string("naïve ✓"), "\"naïve ✓\"");
    let mut o = outcome();
    o.scenario = "we\"ird\\name".into();
    assert!(render(&o, &host()).contains(r#""scenario": "we\"ird\\name","#));
}

#[test]
fn ticks_per_second_never_divides_by_zero() {
    let h = Host {
        elapsed_micros: 0,
        ..host()
    };
    assert_eq!(h.ticks_per_second(10), 10_000_000);
    assert_eq!(host().ticks_per_second(10_000), 5_000);
}
