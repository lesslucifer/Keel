//! E-TESTOPT.W3: the suite is Keel's only integration test program. Cargo builds every `tests/*.rs`,
//! every `tests/<dir>/main.rs` and every `[[test]]` target as a program of its own, and runs the
//! programs one after another, stopping at the first that fails.
use std::path::Path;

/// What cargo reads to find test programs: the names directly under `tests/` (directories with a
/// `main.rs` listed apart), the `.rs` files in `tests/suite/`, its `main.rs` and `Cargo.toml`.
pub struct Layout {
    pub top_files: Vec<String>,
    pub dirs_with_main: Vec<String>,
    pub suite_files: Vec<String>,
    pub suite_main: String,
    pub cargo_toml: String,
}

fn declared_modules(main: &str) -> Vec<String> {
    main.lines()
        .filter_map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("pub ").unwrap_or(l);
            l.strip_prefix("mod ")?.strip_suffix(';').map(|m| m.trim().to_owned())
        })
        .collect()
}

pub fn problems(l: &Layout) -> Vec<String> {
    let mut bad = vec![];
    for f in &l.top_files {
        if let Some(m) = f.strip_suffix(".rs") {
            bad.push(format!(
                "tests/{f} builds as a test program of its own. Move it into the suite: \
                 `git mv tests/{f} tests/suite/{f}`, add `mod {m};` to tests/suite/main.rs, and in \
                 the moved file replace `mod common;` with `use crate::common;`"
            ));
        }
    }
    for d in l.dirs_with_main.iter().filter(|d| d.as_str() != "suite") {
        bad.push(format!(
            "tests/{d}/main.rs builds as a test program of its own. Move each of its files into \
             tests/suite/, declare it with `mod <name>;` in tests/suite/main.rs, and remove tests/{d}/main.rs"
        ));
    }
    let toml: toml::Table = l.cargo_toml.parse().unwrap_or_default();
    if toml.contains_key("test") {
        bad.push("Cargo.toml declares a [[test]] target; the suite (tests/suite/main.rs) is the only test program, so remove it".into());
    }
    if toml.get("package").and_then(|p| p.get("autotests")).is_some() {
        bad.push("Cargo.toml sets package.autotests; with it false cargo never finds the suite, so remove it".into());
    }
    let declared = declared_modules(&l.suite_main);
    for f in &l.suite_files {
        if let Some(m) = f.strip_suffix(".rs") {
            if m != "main" && !declared.iter().any(|d| d == m) {
                bad.push(format!("tests/suite/{f} is not declared in tests/suite/main.rs, so none of its tests run: add `mod {m};`"));
            }
        }
    }
    bad
}

fn read_layout(root: &Path) -> Layout {
    let tests = root.join("tests");
    let (mut top_files, mut dirs_with_main) = (vec![], vec![]);
    for e in std::fs::read_dir(&tests).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            if p.join("main.rs").is_file() {
                dirs_with_main.push(name);
            }
        } else {
            top_files.push(name);
        }
    }
    let suite_files = std::fs::read_dir(tests.join("suite"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    Layout {
        top_files,
        dirs_with_main,
        suite_files,
        suite_main: std::fs::read_to_string(tests.join("suite/main.rs")).unwrap(),
        cargo_toml: std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
    }
}

/// Integration: the real tree builds exactly one integration test program, and every file in it runs.
#[test]
fn the_suite_is_the_only_test_program() {
    let bad = problems(&read_layout(Path::new(env!("CARGO_MANIFEST_DIR"))));
    assert!(bad.is_empty(), "keep Keel's integration tests in one program:\n{}", bad.join("\n"));
}

fn clean() -> Layout {
    Layout {
        top_files: vec![".DS_Store".into(), "README".into()],
        dirs_with_main: vec!["suite".into()],
        suite_files: vec!["main.rs".into(), "cli.rs".into(), "router.rs".into(), "notes.txt".into()],
        suite_main: "//! doc\n#[path = \"../common/mod.rs\"]\nmod common;\n\nmod cli;\npub mod router;\n".into(),
        cargo_toml: "[package]\nname = \"keel\"\n\n[dev-dependencies]\ntempfile = \"3\"\n".into(),
    }
}

/// Unit: each way cargo would build another program, or skip a suite file, is named once with its fix.
#[test]
fn the_layout_check_names_each_extra_program_and_each_undeclared_file() {
    assert_eq!(problems(&clean()), Vec::<String>::new());

    let stray = Layout { top_files: vec!["router_inbox.rs".into()], ..clean() };
    let got = problems(&stray);
    assert_eq!(got.len(), 1, "{got:?}");
    for want in ["tests/router_inbox.rs", "git mv tests/router_inbox.rs tests/suite/router_inbox.rs", "mod router_inbox;", "use crate::common;"] {
        assert!(got[0].contains(want), "{want} missing from {}", got[0]);
    }

    let second = Layout { dirs_with_main: vec!["suite".into(), "slow".into()], ..clean() };
    let got = problems(&second);
    assert!(got.len() == 1 && got[0].starts_with("tests/slow/main.rs builds as a test program"), "{got:?}");

    let undeclared = Layout { suite_files: vec!["main.rs".into(), "cli.rs".into(), "router.rs".into(), "mr.rs".into()], ..clean() };
    let got = problems(&undeclared);
    assert!(got.len() == 1 && got[0].contains("tests/suite/mr.rs is not declared") && got[0].contains("add `mod mr;`"), "{got:?}");

    let target = Layout { cargo_toml: format!("{}\n[[test]]\nname = \"extra\"\npath = \"tests/extra.rs\"\n", clean().cargo_toml), ..clean() };
    let got = problems(&target);
    assert!(got.len() == 1 && got[0].contains("[[test]]"), "{got:?}");

    let off = Layout { cargo_toml: "[package]\nname = \"keel\"\nautotests = false\n".into(), ..clean() };
    let got = problems(&off);
    assert!(got.len() == 1 && got[0].contains("autotests"), "{got:?}");
}
