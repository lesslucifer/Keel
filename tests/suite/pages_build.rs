//! E-GHFLOW2.W8: scripts/pages-build.sh, the GitHub Pages site of every live branch's docs, on a
//! scratch repo with a bare "origin" and a runner's full clone of it, as actions/checkout leaves one.
compiled!();

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

use crate::common;
use crate::router_gh::{commit, git, text, write};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pages-build.sh")
}

/// `pages-build.sh <args>` in `dir`, its scratch under `tmp`, with `env` on top.
fn build_in(dir: &Path, tmp: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = common::cmd("sh");
    c.arg(script()).args(args).current_dir(dir);
    for k in ["PAGES_REMOTE", "RUNNER_TEMP"] {
        c.env_remove(k);
    }
    c.env("RUNNER_TEMP", tmp).envs(env.iter().copied());
    common::hermetic(&mut c);
    c.output().unwrap()
}

fn folder(branch: &str, tmp: &Path) -> Output {
    build_in(tmp, tmp, &["--folder", branch], &[])
}

const ARCH: &str = r##"<html><body><a href="decisions/d.html#dec-1">d</a> <a href="https://example.com/x">out</a> <a href="mailto:a@b">m</a> <a href="#top">top</a></body></html>
"##;
const DEC: &str = r#"<html><body><a href="../architecture.html">back</a></body></html>
"#;
const PLAN: &str = r#"<html><body><a href="../architecture.html?x=1">arch</a> <a href="../decisions/d.html">dec</a></body></html>
"#;
const REPORT: &str = r#"<html><body>epic report <a href="../architecture.html">arch</a> <a href="../work/plan.html">plan</a></body></html>
"#;

/// A developer's repo and its bare origin: main (two linked docs, a README and code outside docs/)
/// → epic/E (a plan) → wu/E/x (its own architecture.html); all pushed.
struct Site {
    root: TempDir,
    dev: PathBuf,
    origin: PathBuf,
    tmp: PathBuf,
}

impl Site {
    fn new() -> Site {
        let root = TempDir::new().unwrap();
        let [dev, origin, tmp] = ["dev", "origin.git", "tmp"].map(|n| root.path().join(n));
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        git(&dev, &["init", "-q", "-b", "main"]);
        write(&dev, "docs/architecture.html", ARCH);
        write(&dev, "docs/decisions/d.html", DEC);
        write(&dev, "README.md", "not a doc\n");
        write(&dev, "src/lib.rs", "// code\n");
        commit(&dev, "main");
        git(&dev, &["switch", "-q", "-c", "epic/E"]);
        write(&dev, "docs/work/plan.html", PLAN);
        commit(&dev, "epic plan");
        git(&dev, &["switch", "-q", "-c", "wu/E/x"]);
        write(&dev, "docs/architecture.html", &ARCH.replace("<body>", "<body>wu x "));
        commit(&dev, "wu x");
        git(root.path(), &["init", "-q", "--bare", "origin.git"]);
        git(&dev, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&dev, &["push", "-q", "--no-verify", "origin", "main", "epic/E", "wu/E/x"]);
        Site { root, dev, origin, tmp }
    }

    fn sha(&self, rev: &str) -> String {
        git(&self.dev, &["rev-parse", rev])
    }

    /// A fresh full clone of origin, as a runner makes one per run.
    fn runner(&self, name: &str) -> PathBuf {
        let r = self.root.path().join(name);
        git(self.root.path(), &["clone", "-q", "--no-checkout", self.origin.to_str().unwrap(), name]);
        r
    }

    /// The builder in a fresh runner clone, into `<root>/<name>-site`; it must succeed.
    fn build(&self, name: &str) -> PathBuf {
        let runner = self.runner(name);
        let site = self.root.path().join(format!("{name}-site"));
        let o = build_in(&runner, &self.tmp, &[site.to_str().unwrap()], &[]);
        assert!(o.status.success(), "{}", text(&o));
        site
    }

    fn show(&self, rev_path: &str) -> Vec<u8> {
        let o = common::cmd("git").args(["show", rev_path]).current_dir(&self.dev).output().unwrap();
        assert!(o.status.success(), "git show {rev_path}");
        o.stdout
    }
}

/// Every file under `dir`, relative, as '/'-joined strings; panics on anything but files and dirs.
fn files(dir: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeSet<String>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                walk(base, &p, out);
            } else {
                assert!(m.is_file(), "{} is neither a file nor a directory", p.display());
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// Every relative href in every page of the site, as (page, href); absolute, mailto, root-relative
/// and fragment-only links are what a browser leaves to another host or the same page.
fn relative_links(site: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in files(site).into_iter().filter(|f| f.ends_with(".html")) {
        let body = read(&site.join(&f));
        for chunk in body.split("href=\"").skip(1) {
            let href = chunk.split('"').next().unwrap();
            if href.contains("://") || href.starts_with("mailto:") || href.starts_with('/') || href.starts_with('#') {
                continue;
            }
            out.push((f.clone(), href.to_string()));
        }
    }
    out
}

/// Where `href` on `page` lands, inside the site, or None when it climbs out of it.
fn resolve(page: &str, href: &str) -> Option<String> {
    let target = href.split(['#', '?']).next().unwrap();
    let mut parts: Vec<&str> = page.split('/').collect();
    parts.pop();
    for p in target.split('/') {
        match p {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

#[test]
fn resolve_climbs_and_strips_fragments() {
    assert_eq!(resolve("a/b/c.html", "../d.html#x").as_deref(), Some("a/d.html"));
    assert_eq!(resolve("a/c.html", "e/f.html?q").as_deref(), Some("a/e/f.html"));
    assert_eq!(resolve("c.html", "../x.html"), None);
}

#[test]
fn each_live_branch_s_docs_land_byte_for_byte_under_its_own_folder_and_nothing_else_does() {
    let s = Site::new();
    let site = s.build("runner");
    let got = files(&site);
    let want: BTreeSet<String> = [
        "index.html",
        "main/index.html",
        "main/docs/architecture.html",
        "main/docs/decisions/d.html",
        "epic/E/index.html",
        "epic/E/docs/architecture.html",
        "epic/E/docs/decisions/d.html",
        "epic/E/docs/work/plan.html",
        "wu/E/x/index.html",
        "wu/E/x/docs/architecture.html",
        "wu/E/x/docs/decisions/d.html",
        "wu/E/x/docs/work/plan.html",
    ]
    .map(String::from)
    .into();
    assert_eq!(got, want, "README.md, src/ and no report note: only docs/ and the indexes");
    for (branch, path) in
        [("main", "docs/architecture.html"), ("epic/E", "docs/work/plan.html"), ("wu/E/x", "docs/architecture.html"), ("wu/E/x", "docs/decisions/d.html")]
    {
        assert_eq!(std::fs::read(site.join(branch).join(path)).unwrap(), s.show(&format!("{branch}:{path}")), "{branch}:{path}");
    }
    assert_ne!(read(&site.join("main/docs/architecture.html")), read(&site.join("wu/E/x/docs/architecture.html")), "each branch shows its own tip");
    let index = read(&site.join("index.html"));
    for (b, tip) in [("main", s.sha("main")), ("epic/E", s.sha("epic/E")), ("wu/E/x", s.sha("wu/E/x"))] {
        assert!(index.contains(&format!(r#"<a href="{b}/index.html">{b}</a> <code>{tip}</code>"#)), "{index}");
    }
    let wu = read(&site.join("wu/E/x/index.html"));
    assert!(wu.contains(r#"<a href="docs/work/plan.html">docs/work/plan.html</a>"#), "{wu}");
    assert!(wu.contains(r#"<a href="../../../index.html">All branches</a>"#), "{wu}");
}

#[test]
fn every_relative_link_in_the_site_resolves_to_a_page_of_the_same_branch() {
    let s = Site::new();
    git(&s.dev, &["notes", "--ref=report", "add", "-m", REPORT.trim_end(), &s.sha("epic/E")]);
    git(&s.dev, &["push", "-q", "--no-verify", "origin", "refs/notes/report"]);
    let site = s.build("runner");
    let all = files(&site);
    let links = relative_links(&site);
    for (page, href) in &links {
        let to = resolve(page, href).unwrap_or_else(|| panic!("{page}: {href} climbs out of the site"));
        assert!(all.contains(&to), "{page}: {href} -> {to}, not in the site");
        let branch_of = |f: &str| ["main/", "epic/E/", "wu/E/x/"].into_iter().find(|b| f.starts_with(b));
        if page != "index.html" && !page.ends_with("/index.html") {
            assert_eq!(branch_of(page), branch_of(&to), "{page}: {href} leaves its branch");
        }
    }
    // not vacuous: the docs' own links, the report's, and each index's were all walked
    for (page, href) in [
        ("wu/E/x/docs/decisions/d.html", "../architecture.html"),
        ("epic/E/docs/work/plan.html", "../decisions/d.html"),
        ("epic/E/docs/_report/report.html", "../work/plan.html"),
        ("epic/E/index.html", "docs/_report/report.html"),
        ("index.html", "wu/E/x/index.html"),
    ] {
        assert!(links.contains(&(page.to_string(), href.to_string())), "{page} -> {href} not walked: {links:?}");
    }
}

#[test]
fn a_deleted_branch_s_folder_is_gone_on_the_next_build_and_an_old_site_is_never_built_over() {
    let s = Site::new();
    let first = s.build("runner1");
    assert!(first.join("wu/E/x/docs/architecture.html").is_file());
    git(&s.dev, &["push", "-q", "--no-verify", "origin", "--delete", "wu/E/x"]);
    let second = s.build("runner2");
    assert!(!second.join("wu").exists(), "{:?}", files(&second));
    assert!(second.join("epic/E/docs/work/plan.html").is_file());
    assert!(!read(&second.join("index.html")).contains("wu/E/x"));
    // the same runner fetching with prune builds the same; the old site is refused, not merged into
    let r1 = s.root.path().join("runner1");
    git(&r1, &["fetch", "-q", "--prune", "origin"]);
    let o = build_in(&r1, &s.tmp, &[first.to_str().unwrap()], &[]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("the out dir must be absent or empty"), "{}", text(&o));
    let third = s.root.path().join("runner1-again");
    let o = build_in(&r1, &s.tmp, &[third.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(files(&third), files(&second));
}

#[test]
fn the_report_note_renders_for_an_epic_tip_only_and_only_from_origin() {
    let s = Site::new();
    for b in ["main", "epic/E", "wu/E/x"] {
        git(&s.dev, &["notes", "--ref=report", "add", "-m", &format!("note on {b}"), &s.sha(b)]);
    }
    // no notes on origin yet: none is published, even one the runner has locally
    let runner = s.runner("runner0");
    git(&runner, &["fetch", "-q", s.dev.to_str().unwrap(), "refs/notes/report:refs/notes/report"]);
    let site0 = s.root.path().join("site0");
    let o = build_in(&runner, &s.tmp, &[site0.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!files(&site0).iter().any(|f| f.contains("_report")), "{:?}", files(&site0));

    git(&s.dev, &["notes", "--ref=report", "add", "-f", "-m", REPORT.trim_end(), &s.sha("epic/E")]);
    git(&s.dev, &["push", "-q", "--no-verify", "origin", "refs/notes/report"]);
    let site = s.build("runner");
    let reports: Vec<String> = files(&site).into_iter().filter(|f| f.contains("_report")).collect();
    assert_eq!(reports, ["epic/E/docs/_report/report.html"]);
    assert_eq!(read(&site.join("epic/E/docs/_report/report.html")), REPORT, "the note's bytes, as stored");
    assert!(read(&site.join("epic/E/index.html")).contains(r#"<a href="docs/_report/report.html">"#));
    assert!(!read(&site.join("main/index.html")).contains("_report"));
}

#[test]
fn links_submodules_scripts_binaries_hidden_reserved_long_and_odd_paths_and_branches_are_skipped_and_logged() {
    let s = Site::new();
    git(&s.dev, &["switch", "-q", "epic/E"]);
    std::os::unix::fs::symlink("/etc/passwd", s.dev.join("docs/leak.html")).unwrap();
    std::os::unix::fs::symlink("../../README.md", s.dev.join("docs/work/readme.html")).unwrap();
    for f in [
        "docs/.hidden.html",
        "docs/.archive/old.html",
        "docs/_report/report.html",
        "docs/we ird.html",
        "docs/caf\u{e9}.html",
        "docs/a@b.html",
        "docs/ok-1_2.v3.html",
        "docs/style.css",
        "docs/img/a.png",
        "docs/img/b.jpg",
        "docs/img/c.jpeg",
        "docs/img/d.gif",
        "docs/img/e.webp",
        "docs/img/f.svg",
        "docs/diagrams/g.drawio",
        "docs/app.js",
        "docs/app.mjs",
        "docs/tool.exe",
        "docs/data.json",
        "docs/bundle.zip",
        "docs/notes.md",
        "docs/UPPER.HTML",
        "docs/noext",
    ] {
        write(&s.dev, f, "<html>x</html>\n");
    }
    write(&s.dev, "docs/run.sh", "#!/bin/sh\necho never run\n");
    crate::router_gh::chmod(&s.dev.join("docs/run.sh"), 0o755);
    git(&s.dev, &["add", "-A"]);
    let main = s.sha("main");
    git(&s.dev, &["update-index", "--add", "--cacheinfo", &format!("160000,{main},docs/sub")]);
    // a part of 200 bytes is published; one of 201, or of 300 (over a file system's 255: written,
    // it would abort the build), is skipped. Made in the index: no file system holds the 300.
    let blob = git(&s.dev, &["hash-object", "-w", "docs/ok-1_2.v3.html"]);
    for n in [195, 196, 295] {
        git(&s.dev, &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},docs/{}.html", "a".repeat(n))]);
    }
    git(&s.dev, &["commit", "-q", "--no-verify", "-m", "odd paths"]);
    for b in ["feature/x", "epic/x@y", "wu/F", "epic-E"] {
        git(&s.dev, &["branch", b, "epic/E"]);
        git(&s.dev, &["push", "-q", "--no-verify", "origin", b]);
    }
    git(&s.dev, &["push", "-q", "--no-verify", "origin", "epic/E"]);

    let runner = s.runner("runner");
    let site = s.root.path().join("site");
    let o = build_in(&runner, &s.tmp, &[site.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", text(&o));
    let got: BTreeSet<String> = files(&site).into_iter().filter(|f| f.starts_with("epic/E/")).collect();
    let long_ok = format!("docs/{}.html", "a".repeat(195));
    let want: BTreeSet<String> = [
        "index.html",
        "docs/architecture.html",
        "docs/decisions/d.html",
        "docs/work/plan.html",
        "docs/ok-1_2.v3.html",
        "docs/style.css",
        "docs/img/a.png",
        "docs/img/b.jpg",
        "docs/img/c.jpeg",
        "docs/img/d.gif",
        "docs/img/e.webp",
        "docs/img/f.svg",
        "docs/diagrams/g.drawio",
        long_ok.as_str(),
    ]
    .map(|f| format!("epic/E/{f}"))
    .into();
    assert_eq!(got, want);
    let tops: BTreeSet<String> = files(&site).iter().map(|f| f.split('/').next().unwrap().to_string()).collect();
    assert_eq!(tops, ["epic", "index.html", "main", "wu"].map(String::from).into(), "no folder for feature/x, epic-E");
    assert!(!site.join("epic/x@y").exists() && !site.join("wu/F").exists());
    let log = String::from_utf8_lossy(&o.stderr).to_string();
    for line in [
        "skipped epic/E:docs/leak.html: mode 120000 is not a regular file",
        "skipped epic/E:docs/work/readme.html: mode 120000 is not a regular file",
        "skipped epic/E:docs/sub: mode 160000 is not a regular file",
        "skipped epic/E:docs/.hidden.html: outside the published paths",
        "skipped epic/E:docs/.archive/old.html: outside the published paths",
        "skipped epic/E:docs/_report/report.html: outside the published paths",
        "skipped epic/E:docs/we ird.html: outside the published paths",
        "skipped epic/E:docs/a@b.html: outside the published paths",
        &format!("skipped epic/E:docs/{}", "a".repeat(100)),
        "skipped epic/E:docs/app.js: not a published file type (html css png jpg jpeg gif webp svg drawio)",
        "skipped epic/E:docs/app.mjs: not a published file type",
        "skipped epic/E:docs/tool.exe: not a published file type",
        "skipped epic/E:docs/data.json: not a published file type",
        "skipped epic/E:docs/bundle.zip: not a published file type",
        "skipped epic/E:docs/notes.md: not a published file type",
        "skipped epic/E:docs/UPPER.HTML: not a published file type",
        "skipped epic/E:docs/noext: not a published file type",
        "skipped epic/E:docs/run.sh: not a published file type",
        "skipped branch feature/x: not main, epic/<E> or wu/<E>/<x>",
        "skipped branch epic/x@y: not main",
        "skipped branch wu/F: not main",
        "skipped branch epic-E: not main",
    ] {
        assert!(log.contains(line), "missing log line {line:?} in:\n{log}");
    }
    assert!(log.contains("skipped epic/E:\"docs/caf"), "a path git quotes is skipped: {log}");
    assert_eq!(log.matches(&format!("docs/{}", "a".repeat(100))).count(), 2, "the 201 and the 300 are skipped, the 200 is not: {log}");
}

#[test]
fn bad_input_is_refused_before_anything_is_written() {
    let s = Site::new();
    let runner = s.runner("runner");
    let full = s.root.path().join("full");
    write(&full, "old.html", "old\n");
    let file = s.root.path().join("a-file");
    std::fs::write(&file, "x").unwrap();
    let link = s.root.path().join("a-link");
    std::os::unix::fs::symlink(s.root.path().join("empty-target"), &link).unwrap();
    std::fs::create_dir(s.root.path().join("empty-target")).unwrap();
    let fresh = s.root.path().join("fresh").to_str().unwrap().to_string();
    type Case<'a> = (Vec<&'a str>, Vec<(&'a str, &'a str)>, &'a str);
    let cases: Vec<Case> = vec![
        (vec![], vec![], "usage:"),
        (vec!["-x"], vec![], "usage:"),
        (vec![&fresh, &fresh], vec![], "usage:"),
        (vec!["--folder"], vec![], "usage:"),
        (vec!["--path"], vec![], "usage:"),
        (vec!["--path", "a", "b"], vec![], "usage:"),
        (vec![&fresh], vec![("PAGES_BRANCH_BUDGET", "1e9")], "PAGES_BRANCH_BUDGET is not a number of bytes"),
        (vec![&fresh], vec![("PAGES_SITE_BUDGET", "12x")], "PAGES_SITE_BUDGET is not a number of bytes"),
        (vec![&fresh], vec![("PAGES_SITE_BUDGET", "-1")], "PAGES_SITE_BUDGET is not a number of bytes"),
        (vec![full.to_str().unwrap()], vec![], "must be absent or empty"),
        (vec![file.to_str().unwrap()], vec![], "is not a directory"),
        (vec![link.to_str().unwrap()], vec![], "is not a directory"),
        (vec![&fresh], vec![("PAGES_REMOTE", "-v")], "PAGES_REMOTE is not a remote name"),
        (vec![&fresh], vec![("PAGES_REMOTE", "a b")], "PAGES_REMOTE is not a remote name"),
        (vec![&fresh], vec![("PAGES_REMOTE", "nowhere")], "cannot read the remote nowhere"),
    ];
    for (args, env, why) in cases {
        let o = build_in(&runner, &s.tmp, &args, &env);
        assert_eq!(o.status.code(), Some(2), "{args:?} {env:?}: {}", text(&o));
        assert!(text(&o).contains(why), "{args:?} {env:?}: want {why:?}, got {}", text(&o));
    }
    assert_eq!(files(&full), ["old.html".to_string()].into(), "a full out dir is left as it was");
    let o = build_in(s.root.path(), &s.tmp, &[&fresh], &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("not in a git repository"), "{}", text(&o));
    assert!(!Path::new(&fresh).exists(), "no out dir is made for a refused run");
    // the scratch directory is removed on every exit
    assert_eq!(std::fs::read_dir(&s.tmp).unwrap().count(), 0);
}

/// The naming rule, as the spec: main, epic/<part>, wu/<part>/<part>; a part is [A-Za-z0-9._-],
/// not starting with a dot, at most 200 bytes; no ".."; at most 400 bytes in all.
fn published(b: &str) -> bool {
    let part = |p: &str| !p.is_empty() && p.len() <= 200 && !p.starts_with('.') && p.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
    let parts: Vec<&str> = b.split('/').collect();
    let shape = match parts.as_slice() {
        ["main"] => true,
        ["epic", e] => part(e),
        ["wu", e, x] => part(e) && part(x),
        _ => false,
    };
    shape && !b.contains("..") && b.len() <= 400
}

#[test]
fn branch_to_folder_maps_every_published_name_to_itself_and_refuses_every_other() {
    let tmp = TempDir::new().unwrap();
    let fixed = [
        ("main", true),
        ("epic/E-GHFLOW2", true),
        ("wu/E-GHFLOW2/w8-pages", true),
        ("epic/_x", true),
        ("wu/E/x.y", true),
        ("main/x", false),
        ("epic/", false),
        ("epic", false),
        ("wu/E", false),
        ("wu/E/x/y", false),
        ("epic/.x", false),
        ("epic/a..b", false),
        ("epic/x y", false),
        ("epic/x\nmain", false),
        ("main\n", false),
        ("feature/x", false),
        ("-main", false),
        ("Main", false),
    ];
    let mut cases: Vec<(String, bool)> = fixed.iter().map(|(b, ok)| (b.to_string(), *ok)).collect();
    let a = |n: usize| "a".repeat(n);
    cases.push((format!("epic/{}", a(200)), true));
    cases.push((format!("epic/{}", a(201)), false));
    cases.push((format!("wu/{}/{}", a(200), a(196)), true));
    cases.push((format!("wu/{}/{}", a(200), a(197)), false));
    cases.push((format!("wu/E/{}", a(201)), false));
    // generated names over an alphabet of the safe set, the separators and bytes it refuses
    let alphabet: Vec<char> = "aZ09._-/@ #%\\:~^?*[\t".chars().collect();
    let heads = ["main", "epic/", "wu/", "wu/E/", "epic/E/", ""];
    let mut next = common::lcg(0x5eed_8a9e);
    for _ in 0..120 {
        let mut b = heads[next(heads.len())].to_string();
        for _ in 0..next(6) {
            b.push(alphabet[next(alphabet.len())]);
        }
        let ok = published(&b);
        cases.push((b, ok));
    }
    assert!(cases.iter().filter(|(_, ok)| *ok).count() >= 10, "the generator reaches published names");
    for (b, ok) in &cases {
        assert_eq!(published(b), *ok, "the spec's own table disagrees for {b:?}");
        let o = folder(b, tmp.path());
        if *ok {
            assert!(o.status.success(), "{b:?}: {}", text(&o));
            assert_eq!(String::from_utf8_lossy(&o.stdout), format!("{b}\n"), "{b:?}");
        } else {
            assert_eq!(o.status.code(), Some(1), "{b:?}: {}", text(&o));
            assert!(o.stdout.is_empty() && o.stderr.is_empty(), "{b:?}: {}", text(&o));
        }
    }
}

#[test]
fn the_path_rule_publishes_only_doc_file_types_under_docs_in_the_safe_set() {
    let tmp = TempDir::new().unwrap();
    let a = |n: usize| "a".repeat(n);
    let cases = [
        ("docs/architecture.html".to_string(), true),
        ("docs/work/x-plan.html".into(), true),
        ("docs/s.css".into(), true),
        ("docs/i/x.png".into(), true),
        ("docs/i/x.jpg".into(), true),
        ("docs/i/x.jpeg".into(), true),
        ("docs/i/x.gif".into(), true),
        ("docs/i/x.webp".into(), true),
        ("docs/i/x.svg".into(), true),
        ("docs/diagrams/x.drawio".into(), true),
        ("docs/-x.html".into(), true),
        (format!("docs/{}.html", a(195)), true),
        (format!("docs/{}/{}.html", a(200), a(185)), true),
        ("docs/x.js".into(), false),
        ("docs/x.exe".into(), false),
        ("docs/x.HTML".into(), false),
        ("docs/x.html.zip".into(), false),
        ("docs/html".into(), false),
        ("docs".into(), false),
        ("docs/".into(), false),
        ("src/x.html".into(), false),
        ("x.html".into(), false),
        ("docs/.x.html".into(), false),
        ("docs/.archive/x.html".into(), false),
        ("docs/_report/report.html".into(), false),
        ("docs/_report".into(), false),
        ("docs/_reports/x.html".into(), true),
        ("docs/a..b.html".into(), false),
        ("docs//x.html".into(), false),
        ("docs/x y.html".into(), false),
        ("docs/x\n.html".into(), false),
        (format!("docs/{}.html", a(196)), false),
        (format!("docs/{}/{}.html", a(200), a(190)), false),
    ];
    for (p, ok) in &cases {
        let o = build_in(tmp.path(), tmp.path(), &["--path", p], &[]);
        assert_eq!(o.status.code(), Some(if *ok { 0 } else { 1 }), "{p:?}: {}", text(&o));
        assert!(o.stdout.is_empty() && o.stderr.is_empty(), "{p:?}: {}", text(&o));
    }
}

#[test]
fn a_branch_over_its_budget_or_past_the_site_s_is_skipped_whole_and_the_rest_still_deploys() {
    let s = Site::new();
    git(&s.dev, &["switch", "-q", "wu/E/x"]);
    write(&s.dev, "docs/big.html", &"x".repeat(20_000));
    commit(&s.dev, "a big doc");
    git(&s.dev, &["push", "-q", "--no-verify", "origin", "wu/E/x"]);
    // each file costs its size rounded up to 4 KiB: main 2 files (8 KiB), epic/E 3 (12 KiB), wu/E/x
    // 3 + big.html at 20 KiB (32 KiB)
    let runner = s.runner("runner");
    let site = s.root.path().join("site-branch");
    let o = build_in(&runner, &s.tmp, &[site.to_str().unwrap()], &[("PAGES_BRANCH_BUDGET", "16384")]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("skipped branch wu/E/x: its files cost 32768 bytes, over the branch budget of 16384"), "{}", text(&o));
    assert!(!site.join("wu").exists());
    assert!(site.join("main/docs/architecture.html").is_file() && site.join("epic/E/docs/work/plan.html").is_file());
    assert!(!read(&site.join("index.html")).contains("wu/E/x"));

    // the site budget: main and epic/E are taken first, whatever the branch names' order
    let site = s.root.path().join("site-total");
    let o = build_in(&runner, &s.tmp, &[site.to_str().unwrap()], &[("PAGES_SITE_BUDGET", "40000")]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("skipped branch wu/E/x: its 32768 bytes would take the site past its budget of 40000 (20480 spent)"), "{}", text(&o));
    assert_eq!(
        files(&site).iter().map(|f| f.split('/').next().unwrap().to_string()).collect::<BTreeSet<_>>(),
        ["epic", "index.html", "main"].map(String::from).into()
    );
    let index = read(&site.join("index.html"));
    assert!(index.find("main/index.html").unwrap() < index.find("epic/E/index.html").unwrap(), "main first: {index}");

    // a report note counts toward its epic's budget
    git(&s.dev, &["notes", "--ref=report", "add", "-m", &"r".repeat(9000), &s.sha("epic/E")]);
    git(&s.dev, &["push", "-q", "--no-verify", "origin", "refs/notes/report"]);
    let runner = s.runner("runner-note");
    let site = s.root.path().join("site-note");
    let o = build_in(&runner, &s.tmp, &[site.to_str().unwrap()], &[("PAGES_BRANCH_BUDGET", "16384")]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("skipped branch epic/E: its files cost 24576 bytes, over the branch budget of 16384"), "{}", text(&o));
    assert!(site.join("main/index.html").is_file() && !site.join("epic").exists());
}
