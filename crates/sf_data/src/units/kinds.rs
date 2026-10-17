//! Stable numbers for the entries of the definition files.
//!
//! The simulation, scenario files, saves and replays name a unit, a weapon, a
//! movement class, an armor type or a damage type by a **number**, the `kind`
//! field of its record. The number is chosen by hand when the record is
//! written, is **permanent**, and does not depend on where the record stands in
//! the file: sorting, grouping or inserting records changes nothing.
//!
//! A record that is removed leaves a **tombstone** in the file's `retired_*`
//! list (`(kind: 12, name: "old_role")`), so that its number is never handed
//! out again. A live record that uses a retired number is refused, and so are
//! two records with the same number.

use serde::Deserialize;

use super::error::{DefError, Source};

/// A removed entry: its number stays taken.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retired {
    /// The number the entry had. Never reused.
    pub kind: u16,
    /// The id or name the entry had, for the human reading the file.
    pub name: String,
}

/// One live entry, for the checks: its number, its name and the key under
/// which the name is written in the file (`id` or `name`).
pub(crate) struct Live<'a> {
    pub kind: u16,
    pub name: &'a str,
}

/// Checks the numbers of one list: no two live entries share a number, no
/// tombstone repeats, no live entry uses a tombstone's number, and every number
/// is at most `max`. `key` is how the live names are written (`"id"` or
/// `"name"`), `what` how errors call an entry.
pub(crate) fn check(
    src: Source<'_>,
    what: &str,
    key: &str,
    max: u16,
    live: &[Live<'_>],
    retired: &[Retired],
) -> Result<(), DefError> {
    for (i, a) in live.iter().enumerate() {
        if a.kind > max {
            return Err(src.error_at_record(
                key,
                a.name,
                0,
                format!(
                    "{what} \"{}\": kind {} is above the largest allowed ({max})",
                    a.name, a.kind
                ),
            ));
        }
        if let Some(other) = live[..i].iter().find(|b| b.kind == a.kind) {
            return Err(src.error_at_record(
                key,
                a.name,
                0,
                format!(
                    "{what} \"{}\": kind {} is already used by \"{}\"",
                    a.name, a.kind, other.name
                ),
            ));
        }
        if let Some(t) = retired.iter().find(|t| t.kind == a.kind) {
            return Err(src.error_at_record(
                key,
                a.name,
                0,
                format!(
                    "{what} \"{}\": kind {} was retired by \"{}\" and is never reused",
                    a.name, a.kind, t.name
                ),
            ));
        }
    }
    for (i, t) in retired.iter().enumerate() {
        if t.kind > max {
            return Err(src.error_at_record(
                "name",
                &t.name,
                0,
                format!(
                    "retired {what} \"{}\": kind {} is above the largest allowed ({max})",
                    t.name, t.kind
                ),
            ));
        }
        if let Some(other) = retired[..i].iter().find(|o| o.kind == t.kind) {
            let nth = retired[..i].iter().filter(|o| o.name == t.name).count();
            return Err(src.error_at_record(
                "name",
                &t.name,
                nth,
                format!(
                    "retired {what} \"{}\": kind {} is already retired by \"{}\"",
                    t.name, t.kind, other.name
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live<'a>(items: &'a [(u16, &'a str)]) -> Vec<Live<'a>> {
        items
            .iter()
            .map(|&(kind, name)| Live { kind, name })
            .collect()
    }

    #[test]
    fn duplicates_and_reused_tombstones_are_refused() {
        let text = "(id: \"a\", id: \"b\", name: \"old\")";
        let src = Source::new("f.ron", text);
        let old = [Retired {
            kind: 7,
            name: "old".into(),
        }];
        assert!(check(src, "unit", "id", 100, &live(&[(1, "a"), (2, "b")]), &old).is_ok());
        let e = check(src, "unit", "id", 100, &live(&[(1, "a"), (1, "b")]), &[]).unwrap_err();
        assert!(e.message.contains("already used by \"a\""), "{e}");
        let e = check(src, "unit", "id", 100, &live(&[(7, "a")]), &old).unwrap_err();
        assert!(e.message.contains("retired by \"old\""), "{e}");
        let twice = [
            old[0].clone(),
            Retired {
                kind: 7,
                name: "older".into(),
            },
        ];
        let e = check(src, "unit", "id", 100, &[], &twice).unwrap_err();
        assert!(e.message.contains("already retired"), "{e}");
        let e = check(src, "unit", "id", 5, &live(&[(6, "a")]), &[]).unwrap_err();
        assert!(e.message.contains("above the largest"), "{e}");
    }
}
