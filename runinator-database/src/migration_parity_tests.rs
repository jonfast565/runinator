//! The three migration directories are maintained separately.
//! A change added to SQLite but not MySQL can pass local tests and fail in production.
//! This test compares their version sets without needing a database.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// versions a single dialect carries alone. each is a fix for something only that engine does, with
/// no counterpart to write elsewhere; anything not listed here must exist in all three.
const DIALECT_ONLY: &[(&str, &str, &str)] = &[
    (
        "postgres",
        "20260527000001",
        "drops bigserial-era id-defaulting triggers that only postgres ever had",
    ),
    (
        "postgres",
        "20260607000002",
        "widens replicas.port to BIGINT; sqlite is untyped and mysql already declared it that way",
    ),
];

fn migrations_dir(dialect: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("migrations")
        .join(dialect)
}

// a migration file is `<version>_<description>.sql`; the version is what sqlx keys on.
fn versions(dialect: &str) -> BTreeSet<String> {
    let dir = migrations_dir(dialect);
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()));

    entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".sql"))
        .map(|name| {
            name.split_once('_')
                .unwrap_or_else(|| panic!("migration {name} is not <version>_<description>.sql"))
                .0
                .to_string()
        })
        .collect()
}

fn allowed_only_in(dialect: &str) -> BTreeSet<String> {
    DIALECT_ONLY
        .iter()
        .filter(|(owner, _, _)| *owner == dialect)
        .map(|(_, version, _)| (*version).to_string())
        .collect()
}

#[test]
fn every_dialect_carries_the_same_migrations() {
    let dialects = ["sqlite", "postgres", "mysql"];
    let sets: Vec<BTreeSet<String>> = dialects.iter().map(|d| versions(d)).collect();

    // the shared set is what every dialect must have: the union minus each dialect's own exemptions.
    let mut shared: BTreeSet<String> = BTreeSet::new();
    for (dialect, set) in dialects.iter().zip(&sets) {
        let exempt = allowed_only_in(dialect);
        shared.extend(set.difference(&exempt).cloned());
    }

    for (dialect, set) in dialects.iter().zip(&sets) {
        let missing: Vec<&String> = shared.difference(set).collect();
        assert!(
            missing.is_empty(),
            "{dialect} is missing migrations {missing:?}; port them or add a DIALECT_ONLY entry \
             saying why they cannot exist there"
        );

        let extra: Vec<&String> = set.difference(&shared).collect();
        let unexplained: Vec<&&String> = extra
            .iter()
            .filter(|version| !allowed_only_in(dialect).contains(**version))
            .collect();
        assert!(
            unexplained.is_empty(),
            "{dialect} carries migrations {unexplained:?} no other dialect has; port them or list \
             them in DIALECT_ONLY with a reason"
        );
    }
}

#[test]
fn dialect_only_exemptions_still_exist() {
    // an exemption whose file was renamed or deleted would silently widen what the parity check
    // tolerates, so the allow-list is itself checked against the tree.
    for (dialect, version, reason) in DIALECT_ONLY {
        assert!(
            versions(dialect).contains(*version),
            "DIALECT_ONLY lists {dialect}/{version} ({reason}) but no such migration exists"
        );
    }
}
