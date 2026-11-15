//! E-TESTOPT.W3: the suite is Keel's only integration test program. Cargo builds every `tests/*.rs`,
//! every `tests/<dir>/main.rs`, every `[[test]]` target and every example or bench marked
//! `test = true` as a program of its own, and runs the programs one after another, stopping at the
//! first that fails. A `.rs` file in `tests/suite/` that rustc never compiled into the suite runs
//! none of its tests; rustc's own record of the files it read (the dep-info file cargo keeps next
//! to the test binary) says which those are, whatever `mod`, `#[path]` or `#[cfg]` decided it.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What sits directly under `tests/`: a name, the source when it is a `.rs` file, and whether a
/// directory holds a `main.rs`.
type Entry = (String, Option<String>, bool);

/// The problems cargo's view of the package shows: test files outside the suite and `Cargo.toml`
/// targets that build another test program.
fn extra_programs(top: &[Entry], suite_names: &BTreeSet<String>, cargo_toml: &str) -> Vec<String> {
    let mut bad = vec![];
    for (f, src, _) in top.iter().filter(|(f, src, _)| src.is_some() && f.ends_with(".rs")) {
        let m = f.trim_end_matches(".rs");
        let mut fix = format!(
            "Move it into the suite: `git mv tests/{f} tests/suite/{f}`, add `mod {m};` to tests/suite/main.rs, \
             and in the moved file replace `mod common;` with `use crate::common;`"
        );
        if matches!(m, "main" | "common") || suite_names.contains(f) {
            fix = format!("Rename it first: tests/suite/{f} exists or `{m}` is taken in the suite, then move it as for any other file");
        }
        let declares = src.as_deref().unwrap_or("").lines().any(|l| {
            let l = l.trim();
            l.starts_with("mod ") && l.ends_with(';') && l != "mod common;"
        });
        if declares {
            fix.push_str(&format!(". It declares modules of its own: move their files to tests/suite/{m}/ with it"));
        }
        bad.push(format!("tests/{f} builds as a test program of its own. {fix}"));
    }
    for (d, _, _) in top.iter().filter(|(d, src, has_main)| src.is_none() && *has_main && d != "suite") {
        bad.push(format!(
            "tests/{d}/main.rs builds as a test program of its own. Move it into the suite: \
             `git mv tests/{d} tests/suite/{d}`, `git mv tests/suite/{d}/main.rs tests/suite/{d}/mod.rs`, add `mod {d};` \
             to tests/suite/main.rs, and in the moved files replace `mod common;` with `use crate::common;`"
        ));
    }
    match cargo_toml.parse::<toml::Table>() {
        Err(e) => bad.push(format!("Cargo.toml does not parse, so the guard cannot see its test targets: {e}")),
        Ok(toml) => {
            if toml.contains_key("test") {
                bad.push("Cargo.toml declares a [[test]] target; the suite (tests/suite/main.rs) is the only test program, so remove it".into());
            }
            if toml.get("package").and_then(|p| p.get("autotests")).is_some() {
                bad.push("Cargo.toml sets package.autotests; with it false cargo never finds the suite, so remove it".into());
            }
            for kind in ["example", "bench"] {
                let targets = toml.get(kind).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                for t in targets.iter().filter(|t| t.get("test").and_then(|v| v.as_bool()) == Some(true)) {
                    let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    bad.push(format!("Cargo.toml marks [[{kind}]] {name} with test = true, so cargo test builds it as a test program; remove the key"));
                }
            }
        }
    }
    bad
}

/// The source files a dep-info file lists, one `path:` rule per file, resolved against `root` when
/// relative, with `..` folded away. Spaces in a path are written `\ `.
fn dep_info_sources(dep_info: &str, root: &Path) -> BTreeSet<PathBuf> {
    dep_info
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.strip_suffix(':'))
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut out = PathBuf::new();
            for c in root.join(p.replace("\\ ", " ")).components() {
                match c {
                    std::path::Component::ParentDir => {
                        out.pop();
                    }
                    std::path::Component::CurDir => {}
                    c => out.push(c),
                }
            }
            out
        })
        .collect()
}

/// Every `.rs` file under `suite` (absolute paths) that the dep-info does not list, with how to
/// make rustc compile it.
fn not_compiled(suite: &Path, files: &[PathBuf], compiled: &BTreeSet<PathBuf>) -> Vec<String> {
    let mut bad = vec![];
    for f in files.iter().filter(|f| !compiled.contains(*f)) {
        let rel = f.strip_prefix(suite).unwrap_or(f);
        let shown = format!("tests/suite/{}", rel.display());
        let dir = rel.parent().filter(|p| !p.as_os_str().is_empty());
        let advice = match dir {
            Some(dir) if rel.file_name().is_some_and(|n| n == "main.rs") => format!(
                "a main.rs is a program's root, not a module: `git mv {shown} tests/suite/{}/mod.rs` and declare `mod {};` \
                 in the module that owns its directory",
                dir.display(),
                dir.file_name().unwrap().to_string_lossy()
            ),
            _ => {
                let name = if rel.file_name().is_some_and(|n| n == "mod.rs") { dir.and_then(|d| d.file_name()) } else { rel.file_stem() };
                format!(
                    "declare it with `mod {};` in the module that owns its directory (tests/suite/main.rs for tests/suite/), \
                     with no #[cfg] that leaves it out on this machine",
                    name.map(|n| n.to_string_lossy()).unwrap_or_default()
                )
            }
        };
        bad.push(format!("{shown} is not compiled into the suite, so none of its tests run: {advice}"));
    }
    bad
}

/// Every `.rs` file under `dir`; a symlinked directory is not entered, so a link loop cannot hang the walk.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if e.file_type().unwrap().is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Integration: the real tree builds exactly one integration test program, and rustc compiled every
/// `.rs` file under `tests/suite/` into this one.
#[test]
fn the_suite_is_the_only_test_program() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
    let tests = root.join("tests");
    let suite = tests.join("suite");
    let mut top = vec![];
    for e in std::fs::read_dir(&tests).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let src = (!p.is_dir() && name.ends_with(".rs")).then(|| std::fs::read_to_string(&p).unwrap());
        top.push((name, src, p.join("main.rs").is_file()));
    }
    let suite_names = std::fs::read_dir(&suite).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    let mut bad = extra_programs(&top, &suite_names, &std::fs::read_to_string(root.join("Cargo.toml")).unwrap());

    let dep_info = std::env::current_exe().unwrap().with_extension("d");
    match std::fs::read_to_string(&dep_info) {
        Err(e) => bad.push(format!(
            "{} cannot be read ({e}): this guard needs the dep-info file cargo writes next to the test binary to see which \
             files rustc compiled; run the suite with cargo test, or a runner that keeps cargo's target layout",
            dep_info.display()
        )),
        Ok(text) => {
            let canonical = |p: PathBuf| p.canonicalize().unwrap_or(p);
            let compiled: BTreeSet<PathBuf> = dep_info_sources(&text, &root).into_iter().map(canonical).collect();
            if compiled.contains(&suite.join("main.rs")) {
                let mut files = vec![];
                rs_files(&suite, &mut files);
                let files: Vec<PathBuf> = files.into_iter().map(canonical).collect();
                bad.extend(not_compiled(&suite, &files, &compiled));
            } else {
                bad.push(format!("{} does not list tests/suite/main.rs, so it is not this suite's dep-info", dep_info.display()));
            }
        }
    }
    assert!(bad.is_empty(), "keep Keel's integration tests in one program:\n{}", bad.join("\n"));
}

const CARGO: &str = "[package]\nname = \"keel\"\n\n[dev-dependencies]\ntempfile = \"3\"\n";

/// Unit: each way cargo would build another program is named once, with a fix that works for it.
#[test]
fn the_layout_check_names_each_extra_program() {
    let suite: BTreeSet<String> = ["main.rs", "cli.rs"].iter().map(|s| s.to_string()).collect();
    let check = |extra: Vec<Entry>| {
        let mut top: Vec<Entry> = vec![("common".into(), None, false), ("suite".into(), None, true), ("README".into(), None, false)];
        top.extend(extra);
        extra_programs(&top, &suite, CARGO)
    };
    assert_eq!(check(vec![]), Vec::<String>::new());
    let file = |n: &str, src: &str| vec![(n.to_string(), Some(src.to_string()), false)];

    let got = check(file("router_inbox.rs", "use std::path::Path;\nmod common;\n"));
    assert_eq!(got.len(), 1, "{got:?}");
    for want in [
        "tests/router_inbox.rs builds as a test program",
        "git mv tests/router_inbox.rs tests/suite/router_inbox.rs",
        "mod router_inbox;",
        "use crate::common;",
    ] {
        assert!(got[0].contains(want), "{want} missing from {}", got[0]);
    }
    assert!(!got[0].contains("modules of its own"), "{}", got[0]);
    for clash in ["main.rs", "common.rs", "cli.rs"] {
        let got = check(file(clash, ""));
        assert!(got.len() == 1 && got[0].contains("Rename it first"), "{clash}: {got:?}");
    }
    let got = check(file("big.rs", "mod common;\nmod helper;\n"));
    assert!(got.len() == 1 && got[0].contains("declares modules of its own: move their files to tests/suite/big/"), "{got:?}");

    let got = check(vec![("slow".into(), None, true)]);
    assert!(got.len() == 1 && got[0].starts_with("tests/slow/main.rs builds as a test program"), "{got:?}");
    assert!(got[0].contains("git mv tests/suite/slow/main.rs tests/suite/slow/mod.rs") && got[0].contains("`mod slow;`"), "{got:?}");

    let cargo = |toml: &str| extra_programs(&[], &suite, toml);
    let got = cargo(&format!("{CARGO}\n[[test]]\nname = \"extra\"\npath = \"tests/extra.rs\"\n"));
    assert!(got.len() == 1 && got[0].contains("[[test]]"), "{got:?}");
    let got = cargo("[package]\nname = \"keel\"\nautotests = false\n");
    assert!(got.len() == 1 && got[0].contains("autotests"), "{got:?}");
    let got = cargo(&format!("{CARGO}\n[[example]]\nname = \"demo\"\ntest = true\n[[example]]\nname = \"plain\"\n[[bench]]\nname = \"speed\"\ntest = true\n"));
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].contains("[[example]] demo with test = true") && got[1].contains("[[bench]] speed with test = true"), "{got:?}");
    let got = cargo("[package\nname = \"keel\"\n[[test]]\nname = \"extra\"\n");
    assert!(got.len() == 1 && got[0].starts_with("Cargo.toml does not parse"), "{got:?}");
}

/// Unit: the dep-info reader takes each `path:` rule, relative or absolute, escaped spaces and `..`
/// included, and nothing from the target rules or the `#` lines.
#[test]
fn the_dep_info_lists_what_rustc_compiled() {
    let text = "/r/target/debug/deps/suite-1.d: tests/suite/main.rs tests/suite/../common/mod.rs\n\n\
                /r/target/debug/deps/suite-1: tests/suite/main.rs\n\n\
                tests/suite/main.rs:\ntests/suite/../common/mod.rs:\n/abs/tests/suite/a\\ b.rs:\n\n\
                # env-dep:CARGO_MANIFEST_DIR=/r\n";
    let got: Vec<PathBuf> = dep_info_sources(text, Path::new("/r")).into_iter().collect();
    let want: Vec<PathBuf> = ["/abs/tests/suite/a b.rs", "/r/tests/common/mod.rs", "/r/tests/suite/main.rs"].iter().map(PathBuf::from).collect();
    assert_eq!(got, want);
}

/// Unit: every suite file the dep-info leaves out is named, with the fix for its shape.
#[test]
fn every_suite_file_rustc_did_not_compile_is_named() {
    let suite = Path::new("/r/tests/suite");
    let f = |p: &str| suite.join(p);
    let files = vec![f("main.rs"), f("cli.rs"), f("off/x.rs"), f("helpers/mod.rs"), f("slow/main.rs"), f("gated.rs")];
    let compiled: BTreeSet<PathBuf> = [f("main.rs"), f("cli.rs"), f("gated.rs")].into_iter().collect();
    let got = not_compiled(suite, &files, &compiled);
    assert_eq!(got.len(), 3, "{got:#?}");
    assert!(got[0].starts_with("tests/suite/off/x.rs is not compiled into the suite") && got[0].contains("`mod x;`"), "{got:#?}");
    assert!(got[1].starts_with("tests/suite/helpers/mod.rs is not compiled") && got[1].contains("`mod helpers;`"), "{got:#?}");
    assert!(got[2].contains("git mv tests/suite/slow/main.rs tests/suite/slow/mod.rs") && got[2].contains("`mod slow;`"), "{got:#?}");
    assert_eq!(not_compiled(suite, &files[..2], &compiled), Vec::<String>::new());
}
