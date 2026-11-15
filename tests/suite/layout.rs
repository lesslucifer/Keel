//! E-TESTOPT.W3: the suite is Keel's only integration test program. Cargo builds every `tests/*.rs`,
//! every `tests/<dir>/main.rs`, every `[[test]]` target and every example or bench marked
//! `test = true` as a program of its own, and runs the programs one after another, stopping at the
//! first that fails. A `.rs` file in `tests/suite/` that no `mod` reaches from `main.rs` is never
//! compiled, so its tests never run.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Every `.rs` file under `tests/`, keyed by its path below `tests/` with `/` separators, the names
/// of the directories directly under `tests/` that hold a `main.rs`, and `Cargo.toml`.
struct Layout {
    files: BTreeMap<String, String>,
    top_names: Vec<String>,
    cargo_toml: String,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
}

/// Rust tokens as far as module declarations need them: comments (nested block comments too),
/// string, byte, raw and char literals are read whole, so a `mod x;` inside one is no declaration.
fn tokens(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    let quoted = |i: &mut usize| -> String {
        let mut s = String::new();
        *i += 1;
        while *i < c.len() && c[*i] != '"' {
            if c[*i] == '\\' {
                *i += 1;
            }
            if *i < c.len() {
                s.push(c[*i]);
            }
            *i += 1;
        }
        *i += 1;
        s
    };
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if ch == '"' {
            out.push(Tok::Str(quoted(&mut i)));
        } else if ch == '\'' {
            if c.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if c.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
            }
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let prefix = matches!(word.as_str(), "r" | "b" | "br" | "c" | "cr");
            if prefix && word.ends_with('r') && matches!(c.get(i), Some('#') | Some('"')) {
                let mut hashes = 0;
                while c.get(i) == Some(&'#') {
                    hashes += 1;
                    i += 1;
                }
                i += 1;
                let start = i;
                while i < c.len() && !(c[i] == '"' && (1..=hashes).all(|k| c.get(i + k) == Some(&'#'))) {
                    i += 1;
                }
                out.push(Tok::Str(c[start..i.min(c.len())].iter().collect()));
                i += 1 + hashes;
            } else if prefix && c.get(i) == Some(&'"') {
                out.push(Tok::Str(quoted(&mut i)));
            } else if word == "b" && c.get(i) == Some(&'\'') {
                i += 1;
                if c.get(i) == Some(&'\\') {
                    i += 1;
                }
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else {
                out.push(Tok::Ident(word));
            }
        } else {
            out.push(Tok::Punct(ch));
            i += 1;
        }
    }
    out
}

/// A `cfg` predicate on this host: `Some(true)` builds, `Some(false)` does not, `None` the guard
/// cannot tell (a feature, or a key it does not know).
fn cfg(t: &[Tok], i: &mut usize) -> Option<bool> {
    let Some(Tok::Ident(name)) = t.get(*i) else { return None };
    *i += 1;
    match (name.as_str(), t.get(*i)) {
        ("any" | "all" | "not", Some(Tok::Punct('('))) => {
            *i += 1;
            let mut vals = vec![];
            while !matches!(t.get(*i), Some(Tok::Punct(')')) | None) {
                vals.push(cfg(t, i));
                if t.get(*i) == Some(&Tok::Punct(',')) {
                    *i += 1;
                }
            }
            *i += 1;
            match name.as_str() {
                "not" => vals.first().copied().flatten().map(|v| !v),
                "any" if vals.contains(&Some(true)) => Some(true),
                "all" if vals.contains(&Some(false)) => Some(false),
                _ if vals.contains(&None) => None,
                "any" => Some(false),
                _ => Some(true),
            }
        }
        (key, Some(Tok::Punct('='))) => {
            let Some(Tok::Str(v)) = t.get(*i + 1) else { return None };
            *i += 2;
            match key {
                "target_os" => Some(v == std::env::consts::OS),
                "target_family" => Some(v == std::env::consts::FAMILY),
                "target_arch" => Some(v == std::env::consts::ARCH),
                _ => None,
            }
        }
        ("unix", _) => Some(cfg!(unix)),
        ("windows", _) => Some(cfg!(windows)),
        ("test", _) => Some(true),
        ("debug_assertions", _) => Some(cfg!(debug_assertions)),
        _ => None,
    }
}

struct ModDecl {
    name: String,
    path: Option<String>,
    built: Option<bool>,
    inline: Vec<String>,
}

/// The `mod name;` items of one file, with their `#[path]`, whether their `#[cfg]` builds, and the
/// inline modules (`mod a { … }`) they sit in.
fn mod_decls(src: &str) -> Vec<ModDecl> {
    let t = tokens(src);
    let mut out = vec![];
    let (mut path, mut built): (Option<String>, Option<bool>) = (None, Some(true));
    let mut inline: Vec<(String, usize)> = vec![];
    let mut depth = 0;
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Tok::Punct('#') => {
                let inner = t.get(i + 1) == Some(&Tok::Punct('!'));
                let open = if inner { i + 2 } else { i + 1 };
                if t.get(open) != Some(&Tok::Punct('[')) {
                    i += 1;
                    continue;
                }
                let mut end = open + 1;
                let mut nest = 1;
                while end < t.len() && nest > 0 {
                    match t[end] {
                        Tok::Punct('[') => nest += 1,
                        Tok::Punct(']') => nest -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if !inner {
                    let a = &t[open + 1..end - 1];
                    match (a.first(), a.get(1), a.get(2)) {
                        (Some(Tok::Ident(k)), Some(Tok::Punct('=')), Some(Tok::Str(p))) if k == "path" => path = Some(p.clone()),
                        (Some(Tok::Ident(k)), Some(Tok::Punct('(')), _) if k == "cfg" => {
                            let mut j = 2;
                            let v = cfg(a, &mut j);
                            built = match (built, v) {
                                (Some(false), _) | (_, Some(false)) => Some(false),
                                (Some(true), Some(true)) => Some(true),
                                _ => None,
                            };
                        }
                        _ => {}
                    }
                }
                i = end;
                continue;
            }
            Tok::Ident(w) if w == "pub" => {
                i += 1;
                if t.get(i) == Some(&Tok::Punct('(')) {
                    while i < t.len() && t[i] != Tok::Punct(')') {
                        i += 1;
                    }
                    i += 1;
                }
                continue;
            }
            Tok::Ident(w) if w == "mod" => {
                if let (Some(Tok::Ident(name)), Some(Tok::Punct(p))) = (t.get(i + 1), t.get(i + 2)) {
                    if *p == ';' {
                        out.push(ModDecl { name: name.clone(), path: path.take(), built, inline: inline.iter().map(|(n, _)| n.clone()).collect() });
                    } else if *p == '{' {
                        depth += 1;
                        inline.push((name.clone(), depth));
                        i += 3;
                        path = None;
                        built = Some(true);
                        continue;
                    }
                }
            }
            Tok::Punct('{') => depth += 1,
            Tok::Punct('}') => {
                if inline.last().is_some_and(|(_, d)| *d == depth) {
                    inline.pop();
                }
                depth -= 1;
            }
            _ => {}
        }
        path = None;
        built = Some(true);
        i += 1;
    }
    out
}

fn normalize(p: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for s in p.split('/') {
        match s {
            "" | "." => {}
            ".." if parts.last().is_some_and(|l| *l != "..") => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn parent(p: &str) -> &str {
    p.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The files the suite compiles, walked from `suite/main.rs` by rustc's rules for `mod x;`, and
/// every declaration whose `#[cfg]` the guard cannot judge.
fn reached(files: &BTreeMap<String, String>) -> (BTreeSet<String>, Vec<String>) {
    let mut seen = BTreeSet::new();
    let mut unknown = vec![];
    let mut todo = vec!["suite/main.rs".to_owned()];
    while let Some(f) = todo.pop() {
        let Some(src) = files.get(&f) else { continue };
        if !seen.insert(f.clone()) {
            continue;
        }
        let dir = parent(&f);
        let stem = f.rsplit('/').next().unwrap().trim_end_matches(".rs");
        let mod_dir = if matches!(stem, "main" | "mod" | "lib") { dir.to_owned() } else { format!("{dir}/{stem}") };
        for d in mod_decls(src) {
            match d.built {
                Some(false) => continue,
                None => {
                    unknown.push(format!("tests/{f} declares `mod {};` under a #[cfg] the guard cannot judge; declare it without one", d.name));
                    continue;
                }
                Some(true) => {}
            }
            let base = if d.inline.is_empty() { mod_dir.clone() } else { format!("{mod_dir}/{}", d.inline.join("/")) };
            match d.path {
                Some(p) if d.inline.is_empty() => todo.push(normalize(&format!("{dir}/{p}"))),
                Some(p) => todo.push(normalize(&format!("{base}/{p}"))),
                None => {
                    todo.push(normalize(&format!("{base}/{}.rs", d.name)));
                    todo.push(normalize(&format!("{base}/{}/mod.rs", d.name)));
                }
            }
        }
    }
    (seen, unknown)
}

fn problems(l: &Layout) -> Vec<String> {
    let mut bad = vec![];
    for (f, src) in l.files.iter().filter(|(f, _)| !f.contains('/')) {
        let m = f.trim_end_matches(".rs");
        let mut fix = format!(
            "Move it into the suite: `git mv tests/{f} tests/suite/{f}`, add `mod {m};` to tests/suite/main.rs, \
             and in the moved file replace `mod common;` with `use crate::common;`"
        );
        if matches!(m, "main" | "common") || l.files.contains_key(&format!("suite/{f}")) {
            fix = format!("Rename it first: tests/suite/{f} exists or `{m}` is taken in the suite, then move it as for any other file");
        }
        let own: Vec<String> = mod_decls(src).into_iter().map(|d| d.name).filter(|n| n != "common").collect();
        if !own.is_empty() {
            fix.push_str(&format!(". It declares its own modules ({}): move them to tests/suite/{m}/ with it", own.join(", ")));
        }
        bad.push(format!("tests/{f} builds as a test program of its own. {fix}"));
    }
    for d in l.top_names.iter().filter(|d| d.as_str() != "suite" && l.files.contains_key(&format!("{d}/main.rs"))) {
        bad.push(format!(
            "tests/{d}/main.rs builds as a test program of its own. Move it into the suite: \
             `git mv tests/{d} tests/suite/{d}`, `git mv tests/suite/{d}/main.rs tests/suite/{d}/mod.rs`, add `mod {d};` \
             to tests/suite/main.rs, and in the moved files replace `mod common;` with `use crate::common;`"
        ));
    }
    match l.cargo_toml.parse::<toml::Table>() {
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
    let (seen, unknown) = reached(&l.files);
    bad.extend(unknown);
    for f in l.files.keys().filter(|f| f.starts_with("suite/") && !seen.contains(*f)) {
        if let Some(d) = f.strip_suffix("/main.rs").filter(|d| d.contains('/')) {
            bad.push(format!(
                "tests/{f} is not reached from tests/suite/main.rs, so none of its tests run: a main.rs is a program's root, \
                 not a module. `git mv tests/{f} tests/{d}/mod.rs` and declare `mod {};` in the module that owns tests/{}",
                d.rsplit('/').next().unwrap(),
                parent(d)
            ));
            continue;
        }
        let name = f.trim_end_matches(".rs").trim_end_matches("/mod").rsplit('/').next().unwrap();
        bad.push(format!(
            "tests/{f} is not reached from tests/suite/main.rs, so it is never compiled and none of its tests run: \
             declare it with `mod {name};` in the module that owns its directory (tests/suite/main.rs for tests/suite/)"
        ));
    }
    bad
}

fn read_layout(root: &Path) -> Layout {
    fn walk(dir: &Path, rel: &str, files: &mut BTreeMap<String, String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let key = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if p.is_dir() {
                walk(&p, &key, files);
            } else if key.ends_with(".rs") {
                files.insert(key, std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    let tests = root.join("tests");
    let mut files = BTreeMap::new();
    walk(&tests, "", &mut files);
    let top_names = std::fs::read_dir(&tests).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    Layout { files, top_names, cargo_toml: std::fs::read_to_string(root.join("Cargo.toml")).unwrap() }
}

/// Integration: the real tree builds exactly one integration test program, and every file in the
/// suite is compiled into it.
#[test]
fn the_suite_is_the_only_test_program() {
    let bad = problems(&read_layout(Path::new(env!("CARGO_MANIFEST_DIR"))));
    assert!(bad.is_empty(), "keep Keel's integration tests in one program:\n{}", bad.join("\n"));
}

const CARGO: &str = "[package]\nname = \"keel\"\n\n[dev-dependencies]\ntempfile = \"3\"\n";

fn layout(files: &[(&str, &str)], cargo: &str) -> Layout {
    let files: BTreeMap<String, String> = files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut top_names: Vec<String> = files.keys().filter_map(|k| k.split_once('/').map(|(d, _)| d.to_owned())).collect();
    top_names.dedup();
    Layout { files, top_names, cargo_toml: cargo.into() }
}

const MAIN: &str = "//! the suite\n#[path = \"../common/mod.rs\"]\nmod common;\n\nmod cli;\n";
const CLEAN: [(&str, &str); 3] = [("common/mod.rs", "pub fn cmd() {}\n"), ("suite/main.rs", MAIN), ("suite/cli.rs", "use crate::common;\n")];

fn with(extra: &[(&str, &str)], main: &str) -> Layout {
    let mut files: Vec<(&str, &str)> = CLEAN.to_vec();
    files.retain(|(k, _)| *k != "suite/main.rs");
    files.push(("suite/main.rs", main));
    files.extend_from_slice(extra);
    layout(&files, CARGO)
}

/// Unit: every way rustc reaches a module counts, and only those: comments, strings and a cfg that
/// does not build reach nothing, and a cfg the guard cannot judge is named.
#[test]
fn the_layout_check_follows_the_module_tree_from_main() {
    assert_eq!(problems(&layout(&CLEAN, CARGO)), Vec::<String>::new());

    let main = format!(
        "{MAIN}mod a; // why\n#[cfg(test)] mod b;\npub(crate) mod c;\n#[cfg(any(unix, windows))]\npub mod d;\n\
         mod outer {{\n    mod inner;\n}}\n#[path = \"elsewhere/e_file.rs\"] mod e;\n"
    );
    let reachable = [
        ("suite/a.rs", "mod nested;\n"),
        ("suite/a/nested.rs", "#[test]\nfn t() {}\n"),
        ("suite/b.rs", ""),
        ("suite/c/mod.rs", "mod d2;\n"),
        ("suite/c/d2.rs", ""),
        ("suite/d.rs", ""),
        ("suite/outer/inner.rs", ""),
        ("suite/elsewhere/e_file.rs", ""),
    ];
    assert_eq!(problems(&with(&reachable, &main)), Vec::<String>::new());

    let hidden = format!(
        "{MAIN}/* mod x;\n/* nested */ mod x2; */\n// mod y;\nconst S: &str = \"mod q;\";\nconst R: &str = r#\"mod r;\"#;\n\
         #[cfg(any())]\nmod z;\n#[cfg(not(test))] mod w;\n"
    );
    let unreached = [
        ("suite/x.rs", ""),
        ("suite/x2.rs", ""),
        ("suite/y.rs", ""),
        ("suite/q.rs", ""),
        ("suite/r.rs", ""),
        ("suite/z.rs", ""),
        ("suite/w.rs", ""),
        ("suite/helpers/mod.rs", ""),
        ("suite/slow/main.rs", ""),
        ("suite/cli/extra.rs", ""),
    ];
    let got = problems(&with(&unreached, &hidden));
    assert_eq!(got.len(), unreached.len(), "{got:#?}");
    for (f, _) in unreached {
        let p = got.iter().find(|p| p.starts_with(&format!("tests/{f} is not reached from tests/suite/main.rs")));
        assert!(p.is_some(), "{f} not named: {got:#?}");
    }
    assert!(got.iter().any(|p| p.starts_with("tests/suite/helpers/mod.rs") && p.contains("`mod helpers;`")), "{got:#?}");
    assert!(got.iter().any(|p| p.starts_with("tests/suite/cli/extra.rs") && p.contains("`mod extra;`")), "{got:#?}");
    assert!(got.iter().any(|p| p.starts_with("tests/suite/slow/main.rs") && p.contains("tests/suite/slow/mod.rs") && p.contains("`mod slow;`")), "{got:#?}");

    let feature = format!("{MAIN}#[cfg(feature = \"slow\")]\nmod f;\n");
    let got = problems(&with(&[("suite/f.rs", "")], &feature));
    assert_eq!(got.len(), 2, "{got:#?}");
    assert!(got[0].contains("declares `mod f;` under a #[cfg] the guard cannot judge"), "{got:#?}");
}

/// Unit: each way cargo would build another program is named once, with a fix that works for it.
#[test]
fn the_layout_check_names_each_extra_program() {
    let stray = |name: &str, src: &str| {
        let mut files = CLEAN.to_vec();
        files.push((name, src));
        problems(&layout(&files, CARGO))
    };
    let got = stray("router_inbox.rs", "use std::path::Path;\nmod common;\n");
    assert_eq!(got.len(), 1, "{got:?}");
    for want in [
        "tests/router_inbox.rs builds as a test program",
        "git mv tests/router_inbox.rs tests/suite/router_inbox.rs",
        "mod router_inbox;",
        "use crate::common;",
    ] {
        assert!(got[0].contains(want), "{want} missing from {}", got[0]);
    }
    assert!(!got[0].contains("its own modules"), "{}", got[0]);
    for clash in ["main.rs", "common.rs", "cli.rs"] {
        let got = stray(clash, "");
        assert!(got.len() == 1 && got[0].contains("Rename it first"), "{clash}: {got:?}");
    }
    let got = stray("big.rs", "mod common;\nmod helper;\n");
    assert!(got.len() == 1 && got[0].contains("declares its own modules (helper): move them to tests/suite/big/"), "{got:?}");

    let got = stray("slow/main.rs", "");
    assert!(got.len() == 1 && got[0].starts_with("tests/slow/main.rs builds as a test program"), "{got:?}");
    assert!(got[0].contains("git mv tests/suite/slow/main.rs tests/suite/slow/mod.rs") && got[0].contains("`mod slow;`"), "{got:?}");

    let cargo = |toml: &str| problems(&layout(&CLEAN, toml));
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

/// Unit: the tokens a module declaration is read from; text inside comments and literals is none.
#[test]
fn the_token_reader_skips_comments_and_literals() {
    let t = tokens("a /* b /* c */ d */ e // f\n\"g\\\"h\" 'i' '\\n' 'life b'j' r##\"k\"#\"## br\"l\" m");
    let want = [Tok::Ident("a".into()), Tok::Ident("e".into()), Tok::Str("g\"h".into()), Tok::Str("k\"#".into()), Tok::Str("l".into()), Tok::Ident("m".into())];
    assert_eq!(t, want);
}
