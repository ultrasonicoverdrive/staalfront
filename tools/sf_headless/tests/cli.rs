//! The `sf_headless` binary as a process: exit codes, output, the log file.

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{TempDir, deterministic_part, scenario_path, string_field};

fn sf_headless(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sf_headless"))
        .args(args)
        .output()
        .expect("start sf_headless")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exited normally")
}

fn mix() -> String {
    scenario_path("command_mix.ron")
        .to_string_lossy()
        .into_owned()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("log file written")
}

#[test]
fn a_run_prints_a_summary_and_writes_the_log() {
    let dir = TempDir::new("cli_ok");
    let log = dir.join("log.json");
    let output = sf_headless(&[&mix(), "--every", "250", "--out", log.to_str().unwrap()]);
    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(stdout.contains("scenario:   command_mix"), "{stdout}");
    assert!(
        stdout.contains("ticks:      1500 (checksum every 250)"),
        "{stdout}"
    );
    assert!(stdout.contains("ticks/s"), "{stdout}");

    let log = read(&log);
    assert!(log.starts_with("{\n  \"format\": 1,\n"), "{log}");
    assert!(log.contains("\"ticks\": 1500,"), "{log}");
    // 250, 500, ..., 1500.
    assert_eq!(log.matches("\"checksum\": ").count(), 6, "{log}");
    assert!(
        stdout.contains(string_field(&log, "final_checksum")),
        "{stdout}"
    );
    assert!(log.contains("\"ticks_per_second\": "), "{log}");
}

#[test]
fn two_runs_write_the_same_checksums() {
    let dir = TempDir::new("cli_twice");
    let (a, b) = (dir.join("a.json"), dir.join("b.json"));
    for out in [&a, &b] {
        let output = sf_headless(&[&mix(), "--every", "100", "--out", out.to_str().unwrap()]);
        assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    }
    let (a, b) = (read(&a), read(&b));
    assert_eq!(deterministic_part(&a), deterministic_part(&b));
}

#[test]
fn ticks_overrides_the_scenario_length() {
    let dir = TempDir::new("cli_ticks");
    let log = dir.join("log.json");
    let output = sf_headless(&[&mix(), "--ticks", "200", "--out", log.to_str().unwrap()]);
    assert_eq!(code(&output), 0);
    let log = read(&log);
    assert!(log.contains("\"ticks\": 200,"), "{log}");
    assert_eq!(log.matches("\"checksum\": ").count(), 2, "{log}"); // 100, 200
}

#[test]
fn the_log_directory_is_created() {
    let dir = TempDir::new("cli_mkdir");
    let log = dir.join("deep/er/log.json");
    let output = sf_headless(&[&mix(), "--ticks", "10", "--out", log.to_str().unwrap()]);
    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert!(log.is_file());
}

#[test]
fn expect_passes_on_a_match() {
    let expected = std::fs::read_to_string(scenario_path("command_mix.expected")).unwrap();
    // The value as `$(cat file)` hands it over on Windows: with a carriage return.
    for value in [
        expected.trim().to_string(),
        format!("{}\r", expected.trim()),
    ] {
        let output = sf_headless(&[&mix(), "--expect", &value]);
        assert_eq!(code(&output), 0, "{}", text(&output.stderr));
        assert!(text(&output.stdout).contains("expect:     ok"));
    }
}

#[test]
fn expect_fails_with_exit_one_on_a_mismatch_and_still_writes_the_log() {
    let dir = TempDir::new("cli_expect");
    let log = dir.join("log.json");
    let output = sf_headless(&[
        &mix(),
        "--expect",
        "0x0123456789abcdef",
        "--out",
        log.to_str().unwrap(),
    ]);
    assert_eq!(code(&output), 1);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("checksum mismatch: expected 0x0123456789abcdef, got 0x"),
        "{stderr}"
    );
    // The evidence is there for the CI artifact.
    assert!(read(&log).contains("\"final_checksum\""));
}

#[test]
fn an_invalid_scenario_exits_two_and_names_the_problem() {
    let dir = TempDir::new("cli_invalid");
    let bad = dir.join("bad.ron");
    std::fs::write(
        &bad,
        "Scenario(seed: 1, ticks: 0, spawns: [\
         (count: 3, kinds: [], owners: [0], area: (min: (0, 0), max: (4, 4)))])",
    )
    .unwrap();
    let output = sf_headless(&[bad.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("bad.ron"), "{stderr}");
    assert!(stderr.contains("ticks must be at least 1"), "{stderr}");
    assert!(stderr.contains("kinds must not be empty"), "{stderr}");
    assert!(text(&output.stdout).is_empty());
}

#[test]
fn an_unknown_field_exits_two() {
    let dir = TempDir::new("cli_unknown");
    let bad = dir.join("typo.ron");
    std::fs::write(&bad, "Scenario(seed: 1, ticks: 5, tcks: 3)").unwrap();
    let output = sf_headless(&[bad.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("tcks"));
}

#[test]
fn a_command_in_the_future_of_the_run_exits_two() {
    let dir = TempDir::new("cli_late");
    let bad = dir.join("late.ron");
    std::fs::write(
        &bad,
        "Scenario(seed: 1, ticks: 5, commands: [\
         (tick: 50, command: Despawn(ids: [Id(0, 0)]))])",
    )
    .unwrap();
    let output = sf_headless(&[bad.to_str().unwrap()]);
    assert_eq!(code(&output), 2);
    assert!(
        text(&output.stderr).contains("never run"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_missing_file_exits_two() {
    let output = sf_headless(&["no/such/scenario.ron"]);
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("no/such/scenario.ron"));
}

#[test]
fn usage_errors_exit_two() {
    for args in [
        vec![],
        vec!["--bogus"],
        vec!["a.ron", "b.ron"],
        vec!["a.ron", "--ticks", "0"],
        vec!["a.ron", "--expect", "nothex"],
    ] {
        let output = sf_headless(&args);
        assert_eq!(code(&output), 2, "{args:?}");
        assert!(text(&output.stderr).contains("Usage:"), "{args:?}");
    }
}

#[test]
fn help_exits_zero() {
    let output = sf_headless(&["--help"]);
    assert_eq!(code(&output), 0);
    assert!(text(&output.stdout).contains("--expect"));
}

#[test]
fn an_unwritable_log_exits_two() {
    let dir = TempDir::new("cli_unwritable");
    // The log path is an existing directory.
    let output = sf_headless(&[
        &mix(),
        "--ticks",
        "5",
        "--out",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("cannot write"));
}
