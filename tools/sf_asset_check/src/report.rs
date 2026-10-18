//! Text and Markdown output of a batch of reports.

use std::fmt::Write as _;

use crate::check::{FileReport, Severity, Status};

/// How many files ended in each status.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
}

impl Totals {
    pub fn of(reports: &[FileReport]) -> Self {
        let mut t = Self::default();
        for r in reports {
            match r.status() {
                Status::Pass => t.pass += 1,
                Status::Warn => t.warn += 1,
                Status::Fail => t.fail += 1,
            }
        }
        t
    }

    pub fn files(&self) -> usize {
        self.pass + self.warn + self.fail
    }

    fn line(&self) -> String {
        format!(
            "{} files: {} pass, {} with warnings only, {} fail",
            self.files(),
            self.pass,
            self.warn,
            self.fail
        )
    }
}

fn slash<T: std::fmt::Display>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn tris(r: &FileReport) -> String {
    let f = &r.facts;
    if f.triangles.is_empty() {
        return "-".into();
    }
    slash(&f.triangles)
}

fn limit(r: &FileReport) -> String {
    r.facts.budget.map_or("-".into(), slash)
}

fn team(r: &FileReport) -> String {
    let f = &r.facts;
    if f.team_share.is_empty() {
        return "-".into();
    }
    slash(f.team_share.iter().map(|s| match s {
        Some(s) => format!("{:.1}", s * 100.0),
        None => "?".into(),
    }))
}

fn size(r: &FileReport) -> String {
    r.facts.bounds.map_or("-".into(), |(lo, hi)| {
        let e = hi - lo;
        format!("{:.2}x{:.2}x{:.2}", e.x, e.y, e.z)
    })
}

fn class(r: &FileReport) -> String {
    let f = &r.facts;
    match f.class {
        None => "?".into(),
        Some(c) if f.pair => format!("{} (pair)", c.name()),
        Some(c) if f.vessel => format!("{} (vessel)", c.name()),
        Some(c) => c.name().into(),
    }
}

fn tag(s: Severity) -> &'static str {
    match s {
        Severity::Warn => "warn",
        Severity::Fail => "FAIL",
    }
}

/// One block per file, then the totals. `quiet` leaves out files without findings.
pub fn text(reports: &[FileReport], quiet: bool) -> String {
    let mut out = String::new();
    for r in reports {
        if quiet && r.status() == Status::Pass {
            continue;
        }
        let _ = writeln!(
            out,
            "{}  {}  [{}]  tris {} (budget {})  materials {}  team% {}  size {} m",
            r.status().label(),
            r.file,
            class(r),
            tris(r),
            limit(r),
            r.facts.materials,
            team(r),
            size(r),
        );
        for f in &r.findings {
            let _ = writeln!(out, "      {}  {}: {}", tag(f.severity), f.rule, f.message);
        }
    }
    let _ = writeln!(out, "{}", Totals::of(reports).line());
    out
}

/// A table of all files and, below it, the findings per file (for a PR body).
pub fn markdown(reports: &[FileReport]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "| file | class | triangles LOD0/1/2 | budget | materials | team % per LOD | size X x Y x Z (m) | result |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
    for r in reports {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |",
            r.file,
            class(r),
            tris(r),
            limit(r),
            r.facts.materials,
            team(r),
            size(r),
            r.status().label(),
        );
    }
    let with_findings: Vec<&FileReport> =
        reports.iter().filter(|r| !r.findings.is_empty()).collect();
    if !with_findings.is_empty() {
        let _ = writeln!(out, "\nFindings:\n");
        for r in with_findings {
            let _ = writeln!(out, "- `{}`", r.file);
            for f in &r.findings {
                let _ = writeln!(
                    out,
                    "  - **{}** `{}`: {}",
                    tag(f.severity),
                    f.rule,
                    f.message
                );
            }
        }
    }
    let _ = writeln!(out, "\n{}", Totals::of(reports).line());
    out
}
