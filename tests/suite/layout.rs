//! E-TESTOPT.W3: the suite is Keel's only integration test program. Cargo builds every `tests/*.rs`,
//! every `tests/<dir>/main.rs`, every `[[test]]` target and every example or bench marked
//! `test = true` as a program of its own, and runs the programs one after another, stopping at the
//! first that fails. A `.rs` file in `tests/suite/` that rustc never compiled into the suite runs
//! none of its tests; rustc's own record of the files it read (the dep-info file cargo keeps next
//! to the test binary) says which those are, whatever `mod`, `#[path]` or `#[cfg]` decided it.
//! rustc opens a file only when its `mod` item survived cfg-stripping; once open, only an inner
//! `#![cfg]` (or a `#![cfg_attr]` that expands to one) drops the whole file, and rustc still lists
//! it. So no suite file may carry one: a cfg goes on the `mod` line, where the dep-info shows it.
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What sits directly under `tests/`: a name, the source when it is a `.rs` file, and whether a
/// directory holds a `main.rs`.
type Entry = (String, Option<String>, bool);

/// The problems cargo's view of the package shows: test files outside the suite and `Cargo.toml`
/// targets that build another test program.
fn extra_programs(top: &[Entry], suite_names: &BTreeSet<String>, cargo_toml: &str) -> Vec<String> {
    let decl = Regex::new(r"(?m)^\s*(#\[[^\]]*\]\s*)*(pub(\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;").unwrap();
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
        if decl.captures_iter(src.as_deref().unwrap_or("")).any(|c| &c[4] != "common") {
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

/// What in one suite file's source would hide tests from the dep-info check: an inner `cfg` or
/// `cfg_attr` attribute (spacing and a path prefix ignored, a comment between `#!` and `[` refused
/// too), and a `.rs` file read as data, which rustc lists like a module.
fn source_problems(shown: &str, src: &str) -> Vec<String> {
    let flat: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    let inner_cfg = Regex::new(r"#!(/\*|\[(::)?([A-Za-z_][A-Za-z0-9_]*::)*cfg(_attr)?\()").unwrap();
    let data = Regex::new(r#"include_(str|bytes)!\(r?#*"[^"]*\.rs""#).unwrap();
    let mut bad = vec![];
    if inner_cfg.is_match(&flat) {
        bad.push(format!(
            "{shown} has an inner #![cfg] or #![cfg_attr]: when it is false the file's tests vanish while rustc still lists \
             the file, so the guard cannot see it; put the cfg on the file's `mod` line instead"
        ));
    }
    if data.is_match(&flat) {
        bad.push(format!("{shown} reads a .rs file with include_str!/include_bytes!, which rustc lists as if compiled; give data another extension"));
    }
    bad
}

/// The directory a dep-info's relative paths start from: cargo writes them relative to the
/// workspace root, which is `root` or one of its ancestors; the one where the listed suite
/// `main.rs` is this suite's.
fn dep_info_base(dep_info: &str, root: &Path, suite_main: &Path) -> Option<PathBuf> {
    let listed = dep_info_sources(dep_info, Path::new(""));
    root.ancestors()
        .find(|base| listed.iter().any(|p| p.is_relative() && base.join(p).canonicalize().is_ok_and(|c| c == suite_main)))
        .map(Path::to_owned)
        .or_else(|| listed.iter().any(|p| p == suite_main).then(|| root.to_owned()))
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
            if let Some(base) = dep_info_base(&text, &root, &suite.join("main.rs")) {
                let compiled: BTreeSet<PathBuf> = dep_info_sources(&text, &base).into_iter().map(canonical).collect();
                let mut files = vec![];
                rs_files(&suite, &mut files);
                let files: Vec<PathBuf> = files.into_iter().map(canonical).collect();
                for f in &files {
                    let shown = format!("tests/suite/{}", f.strip_prefix(&suite).unwrap_or(f).display());
                    bad.extend(source_problems(&shown, &std::fs::read_to_string(f).unwrap()));
                }
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
    for own in ["mod common;\nmod helper;\n", "pub mod helper;\n", "  pub(crate) mod helper;\n", "#[path = \"h.rs\"] mod helper;\n", "mod helper; // note\n"] {
        let got = check(file("big.rs", own));
        assert!(got.len() == 1 && got[0].contains("declares modules of its own: move their files to tests/suite/big/"), "{own:?}: {got:?}");
    }
    for none in ["mod common;\n", "// mod helper;\nfn f() {}\n", "mod inline { }\n"] {
        let got = check(file("big.rs", none));
        assert!(got.len() == 1 && !got[0].contains("modules of its own"), "{none:?}: {got:?}");
    }

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

/// Unit: an inner cfg in any spelling, and a `.rs` file read as data, are refused; an outer cfg on
/// an item, a `#![allow]` and the word cfg in prose are not.
#[test]
fn a_suite_file_may_not_hide_its_tests_from_the_dep_info() {
    let hides = [
        "#%[cfg(any())]\n#[test]\nfn t() {}\n",
        "//! doc\n#% [ cfg (\n    feature = \"slow\"\n) ]\n",
        "#%[cfg_attr(unix, cfg(any()))]\n",
        "#%[core::prelude::v1::cfg(any())]\n",
        "#%/* why */[cfg(any())]\n",
        "mod inline {\n    #%[cfg(windows)]\n}\n",
    ];
    for src in hides {
        let got = source_problems("tests/suite/x.rs", &src.replace("#%", "#!"));
        assert!(got.len() == 1 && got[0].starts_with("tests/suite/x.rs has an inner #!"), "{src:?}: {got:?}");
    }
    for src in ["const T: &str = include_str!(\"fixture.rs\");\n", "const B: &[u8] = include_bytes!( r#\"a/b.rs\"# );\n"] {
        let got = source_problems("tests/suite/x.rs", &src.replace("#%", "#!"));
        assert!(got.len() == 1 && got[0].contains("include_str!/include_bytes!"), "{src:?}: {got:?}");
    }
    for src in ["#%[allow(dead_code)]\n#[cfg(unix)]\n#[test]\nfn t() {}\n", "// a cfg(any()) here is prose\n", "const D: &str = include_str!(\"data.json\");\n"]
    {
        assert_eq!(source_problems("tests/suite/x.rs", &src.replace("#%", "#!")), Vec::<String>::new(), "{src:?}");
    }
}

/// Unit: relative dep-info paths start from the workspace root, the crate's directory or an
/// ancestor of it, whichever makes the listed suite `main.rs` this one; none when no base does.
#[test]
fn the_dep_info_base_is_where_the_listed_suite_is_this_one() {
    let ws = tempfile::TempDir::new().unwrap();
    let ws = ws.path().canonicalize().unwrap();
    let krate = ws.join("crates/keel");
    std::fs::create_dir_all(krate.join("tests/suite")).unwrap();
    let main = krate.join("tests/suite/main.rs");
    std::fs::write(&main, "").unwrap();
    let rule = |p: &str| format!("{p}:\n");
    assert_eq!(dep_info_base(&rule("tests/suite/main.rs"), &krate, &main), Some(krate.clone()));
    assert_eq!(dep_info_base(&rule("crates/keel/tests/suite/main.rs"), &krate, &main), Some(ws.clone()));
    assert_eq!(dep_info_base(&rule(&main.display().to_string()), &krate, &main), Some(krate.clone()));
    assert_eq!(dep_info_base(&rule("other/tests/suite/main.rs"), &krate, &main), None);
}
