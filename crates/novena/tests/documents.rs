//! Keeps the observation documents consistent with each other and with the
//! function table.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Function names the census says were called at least once.
fn called_functions() -> BTreeSet<String> {
    let census = fs::read_to_string(root().join("docs/census/0001-program-a-startup.txt"))
        .expect("census file");
    census
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let calls: u64 = fields.next()?.parse().ok()?;
            let _requested = fields.next()?;
            let name = fields.next()?;
            (calls > 0).then(|| name.to_string())
        })
        .collect()
}

/// Function names that have a row in a signatures table. The tables leave
/// out the API prefix, and the command buffer table also leaves out the
/// object name, so each row is matched against the function table.
fn functions_with_signatures() -> BTreeSet<String> {
    let known: BTreeSet<&str> = novena::functions::all().map(|(_, name)| name).collect();
    let prefix = &novena::functions::all().next().expect("a function").1[..3];
    let mut found = BTreeSet::new();
    let directory = root().join("docs/signatures");
    for entry in fs::read_dir(directory).expect("signatures directory") {
        let path = entry.expect("directory entry").path();
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !file.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("signatures file");
        // Only tables whose rows are functions; other tables (query
        // answers, for one) have other things in their first column.
        if !text.contains("| Function |") {
            continue;
        }
        for line in text.lines() {
            let Some(cell) = line
                .strip_prefix("| ")
                .and_then(|rest| rest.split(" |").next())
            else {
                continue;
            };
            if cell == "Function" || !cell.chars().all(|c| c.is_ascii_alphanumeric()) {
                continue;
            }
            let candidates = [
                format!("{prefix}{cell}"),
                format!("{prefix}CommandBuffer{cell}"),
            ];
            let matched = if file.contains("command-buffer") {
                candidates
                    .iter()
                    .rev()
                    .find(|name| known.contains(name.as_str()))
            } else {
                candidates.iter().find(|name| known.contains(name.as_str()))
            };
            let name =
                matched.unwrap_or_else(|| panic!("{file}: {cell} is not in the function table"));
            found.insert(name.clone());
        }
    }
    found
}

#[test]
fn every_called_function_has_a_signature_entry() {
    let called = called_functions();
    let documented = functions_with_signatures();
    let missing: Vec<_> = called.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "called but not in docs/signatures: {missing:?}"
    );
}

#[test]
fn every_census_name_is_in_the_function_table() {
    for name in called_functions() {
        assert!(novena::functions::lookup(&name).is_some(), "{name}");
    }
}

#[test]
fn stored_reports_never_contain_wide_values() {
    // A hexadecimal value of nine or more digits is wider than 32 bits and
    // could be an address. Shape and census files must not carry any.
    for directory in ["docs/shapes", "docs/census"] {
        for entry in fs::read_dir(root().join(directory)).expect("directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }
            let text = fs::read_to_string(&path).expect("report file");
            for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
                if let Some(digits) = token.strip_prefix("0x") {
                    assert!(digits.len() <= 8, "{}: {token}", path.display());
                }
            }
        }
    }
}
