//! E-TESTOPT.W3: the suite is Keel's only integration test program. Cargo builds every `tests/*.rs`,
//! every `tests/<dir>/main.rs`, every `[[test]]` target and every example or bench marked
//! `test = true` as a program of its own, and runs the programs one after another, stopping at the
//! first that fails.
//!
//! Every `.rs` file under `tests/suite/` proves from inside the built suite that it was compiled:
//! it calls `compiled!()` (tests/suite/main.rs), whose test prints `file!()`. The guard runs those
//! tests in this very binary and names each file that printed nothing. Nothing reads source text,
//! so no spelling of a `mod`, `#[path]`, `#[cfg]` or `cfg_attr` can make a file count without its
//! content in the binary. What it does not prove: that each test inside a compiled file runs (a
//! single test may carry its own `#[cfg]`).
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

compiled!();

/// The prefix of the line each file's `compiled_file` test prints.
pub(crate) const MARK: &str = "suite file compiled: ";

/// What sits directly under `tests/`: a name, whether it is a `.rs` file, and whether a directory
/// holds a `main.rs`.
type Entry = (String, bool, bool);

/// The problems cargo's view of the package shows: test files outside the suite and `Cargo.toml`
/// targets that build another test program.
fn extra_programs(top: &[Entry], suite_names: &BTreeSet<String>, cargo_toml: &str) -> Vec<String> {
    let mut bad = vec![];
    for (f, _, _) in top.iter().filter(|(f, is_rs, _)| *is_rs && f.ends_with(".rs")) {
        let m = f.trim_end_matches(".rs");
        let fix = if matches!(m, "main" | "common") || suite_names.contains(f) {
            format!("Rename it first: tests/suite/{f} exists or `{m}` is taken in the suite, then move it as below")
        } else {
            "Move it into the suite".to_string()
        };
        bad.push(format!(
            "tests/{f} builds as a test program of its own. {fix}: `git mv tests/{f} tests/suite/{f}`, add `mod {m};` to \
             tests/suite/main.rs; if it declares modules of its own, move their files to tests/suite/{m}/ with it; \
             in each moved file, submodules included, replace `mod common;` with `use crate::common;` and add `compiled!();`"
        ));
    }
    for (d, _, _) in top.iter().filter(|(d, is_rs, has_main)| !*is_rs && *has_main && d != "suite") {
        bad.push(format!(
            "tests/{d}/main.rs builds as a test program of its own. Move it into the suite: \
             `git mv tests/{d} tests/suite/{d}`, `git mv tests/suite/{d}/main.rs tests/suite/{d}/mod.rs`, add `mod {d};` \
             to tests/suite/main.rs, and in each moved file replace `mod common;` with `use crate::common;` and add `compiled!();`"
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

fn fold(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| fold(p))
}

/// The files the suite's `compiled_file` tests printed, as canonical paths. `file!()` is relative
/// to where rustc ran: the crate's directory, or a workspace root above it. The base is the one
/// that makes the printed `main.rs` this suite's; `None` when no base does.
fn reported_files(stdout: &str, root: &Path, suite_main: &Path) -> Option<BTreeSet<PathBuf>> {
    let printed: Vec<&Path> = stdout.lines().filter_map(|l| l.split_once(MARK)).map(|(_, f)| Path::new(f)).collect();
    let base = root.ancestors().find(|b| printed.iter().any(|p| canonical(&b.join(p)) == suite_main))?;
    Some(printed.iter().map(|p| canonical(&base.join(p))).collect())
}

/// What this binary's `compiled_file` run reported, or why the guard cannot tell: the run failed,
/// or it passed without reporting `suite_main`.
fn compiled_files(
    passed: bool,
    code: Option<i32>,
    stdout: &str,
    stderr: &str,
    exe: &Path,
    root: &Path,
    suite_main: &Path,
) -> Result<BTreeSet<PathBuf>, String> {
    let exe = exe.display();
    if !passed {
        return Err(format!(
            "{exe} compiled_file failed (exit {code:?}), so the guard cannot tell which files are compiled; fix that run first:\n{stdout}{stderr}"
        ));
    }
    reported_files(stdout, root, suite_main)
        .ok_or_else(|| format!("{exe} compiled_file did not report tests/suite/main.rs, so the guard cannot tell which files are compiled:\n{stdout}{stderr}"))
}

/// The marker tests a `--list` of the suite names: `compiled_file` at the root and in each module, and
/// no other test whose name only contains it, so the guard's child run runs the markers alone.
fn marker_tests(list: &str) -> Vec<String> {
    list.lines().filter_map(|l| l.strip_suffix(": test")).filter(|n| *n == "compiled_file" || n.ends_with("::compiled_file")).map(String::from).collect()
}

/// Unit (E-TESTOPT epic review round 1): the child run's filter takes each marker and nothing else.
#[test]
fn the_guard_runs_only_the_marker_tests() {
    let list = "compiled_file: test\ncli::compiled_file: test\nlayout::a_failed_compiled_file_run_is_told_apart_from_a_silent_one: test\n\
                router::nested::compiled_file: test\nrouter::compiled_file_x: test\n\n5 tests, 0 benchmarks\n";
    assert_eq!(marker_tests(list), ["compiled_file", "cli::compiled_file", "router::nested::compiled_file"]);
}

/// Every `.rs` file under `suite` (canonical paths) that did not report itself, with what to do.
fn not_compiled(suite: &Path, files: &[PathBuf], reported: &BTreeSet<PathBuf>) -> Vec<String> {
    files
        .iter()
        .filter(|f| !reported.contains(*f))
        .map(|f| {
            let shown = format!("tests/suite/{}", f.strip_prefix(suite).unwrap_or(f).display());
            format!(
                "{shown} did not report itself from the built suite, so none of its tests are known to run: it needs \
                 `compiled!();`, and a `mod` that reaches it with no #[cfg] or #![cfg] that leaves it out on this machine \
                 (a main.rs below tests/suite is a program's root: rename it mod.rs). A .rs file that is data, not code, \
                 does not belong under tests/suite"
            )
        })
        .collect()
}

/// Every `.rs` path under `dir`, files and anything else so named; a symlinked directory is not
/// entered, so a link loop cannot hang the walk.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let e = e.unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Integration: the real tree builds exactly one integration test program, and every `.rs` file
/// under `tests/suite/` reports itself from this binary.
#[test]
fn the_suite_is_the_only_test_program() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
    let tests = root.join("tests");
    let suite = tests.join("suite");
    let mut top = vec![];
    for e in std::fs::read_dir(&tests).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        top.push((name, !p.is_dir(), p.join("main.rs").is_file()));
    }
    let suite_names = std::fs::read_dir(&suite).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    let mut bad = extra_programs(&top, &suite_names, &std::fs::read_to_string(root.join("Cargo.toml")).unwrap());

    let exe = std::env::current_exe().unwrap();
    let listed = crate::common::cmd(&exe).arg("--list").output().unwrap();
    let markers = marker_tests(&String::from_utf8_lossy(&listed.stdout));
    assert!(!markers.is_empty(), "{} --list named no compiled_file test: {listed:?}", exe.display());
    let o = crate::common::cmd(&exe).args(&markers).args(["--exact", "--nocapture", "--test-threads=1"]).output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    match compiled_files(o.status.success(), o.status.code(), &stdout, &stderr, &exe, &root, &suite.join("main.rs")) {
        Err(problem) => bad.push(problem),
        Ok(reported) => {
            let mut files = vec![];
            rs_files(&suite, &mut files);
            let files: Vec<PathBuf> = files.iter().map(|p| canonical(p)).collect();
            bad.extend(not_compiled(&suite, &files, &reported));
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
        let mut top: Vec<Entry> = vec![("common".into(), false, false), ("suite".into(), false, true), ("README".into(), true, false)];
        top.extend(extra);
        extra_programs(&top, &suite, CARGO)
    };
    assert_eq!(check(vec![]), Vec::<String>::new());

    let got = check(vec![("router_inbox.rs".into(), true, false)]);
    assert_eq!(got.len(), 1, "{got:?}");
    for want in [
        "tests/router_inbox.rs builds as a test program",
        "Move it into the suite: `git mv tests/router_inbox.rs tests/suite/router_inbox.rs`",
        "mod router_inbox;",
        "use crate::common;",
        "compiled!();",
        "move their files to tests/suite/router_inbox/",
        "in each moved file, submodules included, replace `mod common;`",
    ] {
        assert!(got[0].contains(want), "{want} missing from {}", got[0]);
    }
    for clash in ["main.rs", "common.rs", "cli.rs"] {
        let got = check(vec![(clash.into(), true, false)]);
        assert!(got.len() == 1 && got[0].contains("Rename it first"), "{clash}: {got:?}");
    }

    let got = check(vec![("slow".into(), false, true)]);
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

/// Unit: the printed files (after libtest's `test … ... ` on the same line) resolve from the crate's directory or a workspace root above it, whichever
/// makes the printed `main.rs` this suite's; lines without the mark are not reports; with no
/// `main.rs` of this suite printed there is no answer.
#[test]
fn the_reports_resolve_from_where_rustc_ran() {
    let ws = tempfile::TempDir::new().unwrap();
    let ws = ws.path().canonicalize().unwrap();
    let krate = ws.join("crates/keel");
    std::fs::create_dir_all(krate.join("tests/suite")).unwrap();
    std::fs::create_dir_all(krate.join("tests/common")).unwrap();
    let main = krate.join("tests/suite/main.rs");
    for f in ["tests/suite/main.rs", "tests/suite/cli.rs", "tests/common/mod.rs"] {
        std::fs::write(krate.join(f), "").unwrap();
    }
    let out = |lines: &[&str]| lines.iter().map(|l| format!("test x::compiled_file ... {MARK}{l}\nok\n")).collect::<String>();
    let want: BTreeSet<PathBuf> = [main.clone(), krate.join("tests/suite/cli.rs")].into_iter().collect();

    let crate_rel = format!("running 2 tests\n{}test result: ok\n", out(&["tests/suite/main.rs", "tests/suite/cli.rs"]));
    assert_eq!(reported_files(&crate_rel, &krate, &main), Some(want.clone()));
    let ws_rel = out(&["crates/keel/tests/suite/main.rs", "crates/keel/tests/suite/cli.rs"]);
    assert_eq!(reported_files(&ws_rel, &krate, &main), Some(want.clone()));
    let abs = out(&[&main.display().to_string(), &format!("{}/tests/suite/../suite/cli.rs", krate.display())]);
    assert_eq!(reported_files(&abs, &krate, &main), Some(want));
    let common = out(&["tests/suite/main.rs", "tests/suite/../common/mod.rs"]);
    assert!(reported_files(&common, &krate, &main).unwrap().contains(&krate.join("tests/common/mod.rs")));

    assert_eq!(reported_files("tests/suite/main.rs\n", &krate, &main), None, "an unmarked line is no report");
    assert_eq!(reported_files(&out(&["other/tests/suite/main.rs"]), &krate, &main), None);
}

/// Unit: every suite file that did not report itself is named; a reported one is not.
#[test]
fn every_suite_file_that_did_not_report_itself_is_named() {
    let suite = Path::new("/r/tests/suite");
    let files = vec![suite.join("main.rs"), suite.join("cli.rs"), suite.join("off/x.rs"), suite.join("slow/main.rs")];
    let reported: BTreeSet<PathBuf> = [suite.join("main.rs"), suite.join("cli.rs")].into_iter().collect();
    let got = not_compiled(suite, &files, &reported);
    assert_eq!(got.len(), 2, "{got:#?}");
    assert!(got[0].starts_with("tests/suite/off/x.rs did not report itself from the built suite") && got[0].contains("`compiled!();`"), "{got:#?}");
    assert!(got[1].starts_with("tests/suite/slow/main.rs did not report itself") && got[1].contains("rename it mod.rs"), "{got:#?}");
    assert_eq!(not_compiled(suite, &files[..2], &reported), Vec::<String>::new());
}

/// Unit: `..` drops the component before it and `.` is skipped, for a path that does not exist.
#[test]
fn fold_resolves_dot_dot_and_dot_without_the_file_system() {
    assert_eq!(fold(Path::new("/r/tests/suite/../common/./mod.rs")), Path::new("/r/tests/common/mod.rs"));
    assert_eq!(fold(Path::new("/r/a/b/../../c")), Path::new("/r/c"));
}

/// Unit: a `compiled_file` run that failed is named as a failure with its output; a run that
/// passed without reporting `main.rs` is named as that; a run that reported gives the files.
#[test]
fn a_failed_compiled_file_run_is_told_apart_from_a_silent_one() {
    let (root, main, exe) = (Path::new("/r"), Path::new("/r/tests/suite/main.rs"), Path::new("/r/target/suite-x"));
    let report = format!("test compiled_file ... {MARK}tests/suite/main.rs\nok\n");
    let failed = compiled_files(false, Some(101), &report, "boom", exe, root, main).unwrap_err();
    assert!(failed.starts_with("/r/target/suite-x compiled_file failed (exit Some(101))"), "{failed}");
    assert!(failed.contains("boom") && !failed.contains("did not report"), "{failed}");
    let silent = compiled_files(true, Some(0), "running 0 tests\n", "", exe, root, main).unwrap_err();
    assert!(silent.contains("did not report tests/suite/main.rs") && !silent.contains("failed"), "{silent}");
    assert_eq!(compiled_files(true, Some(0), &report, "", exe, root, main).unwrap(), [main.to_path_buf()].into_iter().collect());
}
