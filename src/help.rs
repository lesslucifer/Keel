//! `--help` and `-h` on any keel command (E-KEELFIX.W2): print that command's usage, exit 0, and
//! write nothing. Checked in `main()` before anything else runs - before `--version`, the repository
//! lookup and `mr`'s checkout check - and winning over every other argument, so a help request can
//! never sign, commit or write a note.

use crate::{gate, mr, notes};

/// `run()`'s own usage errors print these; `tests/suite/verify_dod.rs` keeps the copies equal.
const TOP: &str = "usage: keel <init [-f] | lint | check | verify-doc | verify-dod | preflight | mr <setup|commit|confirm> | hooks install [--force] | notes write <test|doc|cr>>";
const INIT: &str = "usage: keel init [--force | -f]";
const HOOKS: &str = "usage: keel hooks install [--force]";
const LINT: &str = "usage: keel lint";
const CHECK: &str = "usage: keel check";
const HOOK: &str = "usage: keel hook <pre-commit | commit-msg <file> | pre-push <remote> <url>>\n       (git's hooks run these; you do not)";
const VERIFY_DOC: &str = "usage: keel verify-doc [<path>...]";

/// Each command's leading words and its usage; the longest match wins, no match is the top usage.
/// Every command keel dispatches has its own row here: the test
/// `every_dispatched_command_has_its_own_usage` reads every module's dispatch and fails closed on
/// what it cannot read (its rules are written above `SOURCES` in the tests below).
const TABLE: &[(&[&str], &str)] = &[
    (&[], TOP),
    (&["init"], INIT),
    (&["hooks"], HOOKS),
    (&["hooks", "install"], HOOKS),
    (&["lint"], LINT),
    (&["check"], CHECK),
    (&["hook"], HOOK),
    (&["hook", "pre-commit"], HOOK),
    (&["hook", "commit-msg"], HOOK),
    (&["hook", "pre-push"], HOOK),
    (&["verify-doc"], VERIFY_DOC),
    (&["verify-dod"], gate::USAGE),
    (&["verify-dod", "commit"], gate::USAGE),
    (&["verify-dod", "hook"], gate::USAGE),
    (&["verify-dod", "range"], gate::USAGE),
    (&["preflight"], gate::USAGE),
    (&["notes"], notes::NOTES_USAGE),
    (&["notes", "write"], notes::NOTES_USAGE),
    (&["notes", "write", "test"], notes::TEST_USAGE),
    (&["notes", "write", "doc"], notes::DOC_USAGE),
    (&["notes", "write", "cr"], notes::CR_USAGE),
    (&["mr"], mr::MR_USAGE),
    (&["mr", "setup"], mr::SETUP_USAGE),
    (&["mr", "commit"], mr::COMMIT_USAGE),
    (&["mr", "confirm"], mr::CONFIRM_USAGE),
];

fn is_help(a: &str) -> bool {
    a == "--help" || a == "-h"
}

/// The usage to print when any argument is exactly `--help` or `-h`; `None` otherwise. The command
/// is read from the other arguments' leading words, up to the first that starts with `-`. Git runs
/// `keel hook <name> <git's arguments>`, and a remote may be named `-h` or `--help`: every word after
/// a hook's name is git's, never a help request, so the hook always runs (review round 1, B1).
pub(crate) fn usage_for(args: &[&str]) -> Option<&'static str> {
    let gits = args.first() == Some(&"hook") && args.get(1).is_some_and(|a| !is_help(a));
    if gits || !args.iter().any(|a| is_help(a)) {
        return None;
    }
    let words: Vec<&str> = args.iter().copied().filter(|a| !is_help(a)).take_while(|a| !a.starts_with('-')).collect();
    TABLE.iter().filter(|(k, _)| words.starts_with(k)).max_by_key(|(k, _)| k.len()).map(|(_, u)| *u)
}

/// A help request: print the usage on stdout and exit 0. Otherwise return, and keel runs.
pub(crate) fn exit_if_asked() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(usage) = usage_for(&args.iter().map(String::as_str).collect::<Vec<_>>()) {
        println!("{usage}");
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{usage_for, TABLE, TOP};
    use crate::{gate, mr, notes};

    /// A real command line per usage, with the values its flags take.
    const LINES: &[(&[&str], &str)] = &[
        (&[], TOP),
        (&["init", "-f"], super::INIT),
        (&["hooks", "install", "--force"], super::HOOKS),
        (&["lint"], super::LINT),
        (&["check"], super::CHECK),
        (&["hook"], super::HOOK),
        (&["verify-doc", "docs/a.html"], super::VERIFY_DOC),
        (&["verify-dod", "wu", "wu/e/a", "--target", "epic/e", "--no-receipt"], gate::USAGE),
        (&["preflight", "epic", "epic/e"], gate::USAGE),
        (&["notes", "write"], notes::NOTES_USAGE),
        (&["notes", "write", "test", "--target", "epic/e"], notes::TEST_USAGE),
        (&["notes", "write", "doc", "--distill", "docs/work/p.html=docs/d.html"], notes::DOC_USAGE),
        (&["notes", "write", "cr", "--verdict", "PASS", "--fixes", "abc"], notes::CR_USAGE),
        (&["mr", "bogus"], mr::MR_USAGE),
        (&["mr", "setup", "--as", "vu", "--key", "k"], mr::SETUP_USAGE),
        (&["mr", "commit", "-m", "msg", "--as", "vu"], mr::COMMIT_USAGE),
        (&["mr", "confirm", "wu", "wu/e/a", "--as", "vu", "--report", "r.html"], mr::CONFIRM_USAGE),
    ];

    #[test]
    fn no_help_word_is_no_help() {
        for (line, _) in LINES {
            assert_eq!(usage_for(line), None, "{line:?}");
        }
        for not_help in ["--help=x", "-hv", "-H", "help", "--helpful", "-help"] {
            assert_eq!(usage_for(&["mr", "confirm", "wu", not_help]), None, "{not_help}");
        }
    }

    /// The invariant: either help word, inserted at any position of any command line, gives that
    /// command's usage - help wins over every other argument, a flag's value included.
    #[test]
    fn help_anywhere_on_a_line_gives_that_lines_usage() {
        for (line, usage) in LINES {
            for help in ["--help", "-h"] {
                for at in 0..=line.len() {
                    let mut args = line.to_vec();
                    args.insert(at, help);
                    assert_eq!(usage_for(&args), Some(*usage), "{args:?}");
                }
            }
        }
    }

    #[test]
    fn an_unknown_command_or_a_leading_flag_gives_the_top_usage() {
        for args in [&["--version", "--help"][..], &["-V", "-h"], &["bogus", "--help"], &["--help"], &["-h", "-h"]] {
            assert_eq!(usage_for(args), Some(TOP), "{args:?}");
        }
        assert_eq!(usage_for(&["mr", "commit", "-m", "--help"]), Some(mr::COMMIT_USAGE), "a message reading --help is help");
        assert_eq!(usage_for(&["notes", "write", "report", "-h"]), Some(notes::NOTES_USAGE), "an unknown kind: the writer's usage");
    }

    #[test]
    fn every_usage_names_its_own_command() {
        for (words, usage) in TABLE {
            let first = usage.lines().next().unwrap();
            assert!(first.starts_with("usage: keel"), "{first}");
            let cmd = words.iter().take(1).chain(words.iter().skip(1).filter(|w| **w != "write")).copied().collect::<Vec<_>>();
            for w in cmd {
                assert!(usage.contains(w), "{words:?}: {usage}");
            }
        }
    }

    /// Round 1 blocker: after a hook's name every word is git's (a remote named `-h`), never help.
    #[test]
    fn a_hooks_arguments_are_gits_never_help() {
        for args in [&["hook", "pre-push", "-h"][..], &["hook", "pre-push", "--help", "url"], &["hook", "commit-msg", "-h"], &["hook", "pre-commit", "--help"]]
        {
            assert_eq!(usage_for(args), None, "{args:?}");
        }
        for args in [&["hook", "-h"][..], &["hook", "--help", "pre-push"], &["-h", "hook", "pre-push", "origin"]] {
            assert_eq!(usage_for(args), Some(super::HOOK), "{args:?}: a person asking");
        }
    }

    // ── The dispatch guard (review rounds 1-3) ─────────────────────────────────────────────────
    // It reads the whole program's production code and fails closed: a command keel dispatches is
    // read and must have its own TABLE row, and a shape it cannot read is an error, never skipped.
    //
    // Read: every module `main.rs` declares and every module those declare (`mod x;`, `pub mod x;`,
    // `pub(crate) mod x;`), each cut at its `#[cfg(test)]` + `mod tests` (an item-level
    // `#[cfg(test)]` fn is read). A literal word is a string literal that starts with a letter or
    // digit (a flag is no command).
    //
    // Refused anywhere in that code:
    //   - a declared module the guard does not read;
    //   - a slice pattern holding a literal word (`[..] =>` arms, wrapped `| [..] =>` lines,
    //     `let [`, `if let [`, `matches!(.., [..])`) outside every dispatcher in DISPATCHERS;
    //   - `Some([`, `Ok([`, `Err([`, `@ [` or `@ (` on a line holding a literal word.
    //
    // An argv reader is a dispatcher, or any fn with an `args`, `rest`, `flags` or `paths`
    // parameter of type `&[&str]`. In one, every literal word is refused by default unless it is
    //   (a) read: a leading word of a slice pattern (an arm, `let [`, `matches!(.., [..])`), the
    //       head of a string arm (`"w" =>`, `"a" | "b" =>`), or `w` in `args.first() == Some(&"w")`;
    //   (b) a plain value: not in an arm's head or guard, not in a `let` pattern, not inside
    //       `matches!`, not next to `==` or `!=`, and not the argument of `Some(`, `Ok(`, `Err(` or
    //       a comparing method (`.contains(`, `.starts_with(`, `.ends_with(`, `.eq(`, `.ne(`,
    //       `.strip_prefix(`, `.strip_suffix(`, `.find(`, `.rfind(`, `.matches(`); or
    //   (c) on a reviewed line in NOT_COMMANDS.
    // So a word bound then compared in an arm's guard (`[w, ..] if *w == "x" =>`), `Some("x")` in
    // an arm or an `if let`, a tuple arm, a word after a binding in a slice, a const, tuple or nested
    // element, and a `[` with no `]` are each refused.
    //
    // A read word is placed under the dispatcher's words, or under its enclosing arm's words when
    // that arm's body is `match <name> {`; an arm nested any other way is refused. Known limit: a
    // word reaching a dispatch through a variable, or through a fn that is not an argv reader
    // (e.g. `gate::level_of(s)`, whose levels are values, not commands).

    /// Every module the program compiles, as compiled.
    const SOURCES: &[(&str, &str)] = &[
        ("main.rs", include_str!("main.rs")),
        ("gate.rs", include_str!("gate.rs")),
        ("help.rs", include_str!("help.rs")),
        ("init.rs", include_str!("init.rs")),
        ("mr.rs", include_str!("mr.rs")),
        ("notes.rs", include_str!("notes.rs")),
        ("review.rs", include_str!("review.rs")),
        ("verify_doc.rs", include_str!("verify_doc.rs")),
    ];

    /// Where keel dispatches a command: the file, the fn, and the words that lead to it.
    const DISPATCHERS: &[(&str, &str, &[&str])] = &[
        ("main.rs", "run", &[]),
        ("gate.rs", "verify_dod", &["verify-dod"]),
        ("gate.rs", "preflight_cmd", &["preflight"]),
        ("mr.rs", "run", &["mr"]),
        ("notes.rs", "write", &["notes", "write"]),
        ("notes.rs", "known_args", &["notes", "write"]),
    ];

    /// Reviewed lines that match or compare a literal word yet select no command: file, text, why.
    const NOT_COMMANDS: &[(&str, &str, &str)] = &[
        ("mr.rs", r#"if let ["100644" | "100755", "blob", id]"#, "a git ls-tree line, not argv"),
        ("mr.rs", r#""wu" => false,"#, "mr confirm's level, a value"),
        ("mr.rs", r#""epic" => true,"#, "mr confirm's level, a value"),
        ("mr.rs", r#""HEAD" => git("#, "mr confirm's tip, a value"),
        ("mr.rs", r#"tip_ref != "HEAD""#, "mr confirm's tip, a value"),
        ("mr.rs", r#"if tier == "key""#, "mr confirm's signer tier, from the roster"),
        ("notes.rs", r#"verdict != "PASS""#, "the cr writer's --verdict value"),
        ("help.rs", r#"args.first() == Some(&"hook")"#, "help's exemption for git's hook arguments; `hook` has its row"),
        ("mr.rs", r#"[t, b, ..] if ["ssh-", "ecdsa-", "sk-"]"#, "mr setup reads a public key's type prefix, not argv"),
        (
            "verify_doc.rs",
            r#"[_] | ["epic-plan", "wu-plan"] | ["epic-result", "wu-result"] => None,"#,
            "a doc's sorted artifact roles: a one-part epic's pairs, not argv",
        ),
    ];

    /// The string literals on a line: the offset of each opening quote, and the text between.
    fn literals(line: &str) -> Vec<(usize, &str)> {
        let (mut out, mut start, mut esc) = (vec![], None, false);
        for (i, c) in line.char_indices() {
            match (start, c) {
                (Some(_), '\\') if !esc => esc = true,
                (Some(s), '"') if !esc => {
                    out.push((s - 1, &line[s..i]));
                    start = None;
                }
                (None, '"') => start = Some(i + 1),
                _ => esc = false,
            }
        }
        out
    }

    fn word(lit: &str) -> bool {
        lit.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && lit.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
    }

    /// Where `=>` stands outside every string literal, if anywhere.
    fn arrow(t: &str) -> Option<usize> {
        let lits = literals(t);
        let mut from = 0;
        while let Some(i) = t[from..].find("=>").map(|i| i + from) {
            if !lits.iter().any(|(q, c)| i > *q && i <= q + c.len() + 1) {
                return Some(i);
            }
            from = i + 2;
        }
        None
    }

    const BEFORE: &[&str] = &[
        "==",
        "!=",
        "Some(",
        "Ok(",
        "Err(",
        ".contains(",
        ".starts_with(",
        ".ends_with(",
        ".eq(",
        ".ne(",
        ".strip_prefix(",
        ".strip_suffix(",
        ".find(",
        ".rfind(",
        ".matches(",
    ];
    const AFTER: &[&str] = &["==", "!=", ".eq(", ".ne("];

    /// Whether the literal whose opening quote is at `q` is compared, not a plain value.
    fn compared(t: &str, q: usize, len: usize) -> bool {
        let before = t[..q].trim_end_matches(['&', '*', ' ']);
        let after = t[q + len + 2..].trim_start();
        BEFORE.iter().any(|b| before.ends_with(b)) || AFTER.iter().any(|a| after.starts_with(a))
    }

    /// The slice pattern(s) at the start of `t` (`[..] | [..]` alternatives, `"a" | "b"` elements).
    struct Slice {
        /// The leading literal words of each; a flag, a binding or `..` ends a path.
        paths: Vec<Vec<String>>,
        /// Where the patterns end in `t`.
        end: usize,
        /// A literal word after a path ended (`[x, "w"]`): matched, but not as a command path.
        stray: bool,
    }

    /// `Err` for an element it cannot read: a const, a tuple or a nested slice, or no `]`.
    fn slice_paths(t: &str) -> Result<Slice, String> {
        let (mut paths_all, mut stray, mut rest_all) = (vec![], false, t);
        while let Some(rest) = rest_all.strip_prefix('[') {
            let end = rest.find(']').ok_or("a slice pattern with no `]` on its line")?;
            let mut paths = vec![vec![]];
            let mut open = true;
            for e in rest[..end].split(',').map(str::trim).filter(|e| !e.is_empty()) {
                let alts: Vec<&str> = e.split('|').map(str::trim).collect();
                let lit = |a: &&str| a.len() > 2 && a.starts_with('"') && a.ends_with('"');
                if alts.iter().any(|a| a.starts_with(['(', '[']) || a.chars().next().is_some_and(|c| c.is_ascii_uppercase())) {
                    return Err(format!("an element it cannot read: {e}"));
                }
                if open && alts.iter().all(lit) && !alts.iter().any(|a| a.starts_with("\"-")) {
                    paths = paths
                        .iter()
                        .flat_map(|p: &Vec<String>| alts.iter().map(move |a| [p.clone(), vec![a.trim_matches('"').to_string()]].concat()))
                        .collect();
                } else {
                    open = false;
                    stray |= alts.iter().any(|a| lit(a) && word(a.trim_matches('"')));
                }
            }
            paths_all.extend(paths.into_iter().filter(|p| !p.is_empty()));
            rest_all = rest[end + 1..].trim_start();
            match rest_all.strip_prefix('|').map(str::trim_start) {
                Some(next) if next.starts_with('[') => rest_all = next,
                _ => break,
            }
        }
        Ok(Slice { paths: paths_all, end: t.len() - rest_all.len(), stray })
    }

    /// A fn in production code: its name, its line span, the words it dispatches under (when it is
    /// a dispatcher) and whether it reads argv.
    struct Fn {
        name: String,
        lines: std::ops::Range<usize>,
        under: Option<&'static [&'static str]>,
        argv: bool,
    }

    fn fns(file: &str, lines: &[&str]) -> Vec<Fn> {
        let mut out = vec![];
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim_start();
            let indent = l.len() - t.len();
            let t = ["pub(crate) ", "pub "].iter().fold(t, |t, p| t.strip_prefix(p).unwrap_or(t));
            let Some(sig) = t.strip_prefix("fn ") else { continue };
            let name: String = sig.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let close = format!("{}}}", " ".repeat(indent));
            let Some(end) = lines[i..].iter().position(|l| *l == close).map(|n| i + n) else { continue };
            let head: String = lines[i..=end]
                .iter()
                .take_while(|l| !l.ends_with('{'))
                .chain(lines.get(i + lines[i..].iter().position(|l| l.ends_with('{')).unwrap_or(0)))
                .copied()
                .collect();
            let argv = ["args", "rest", "flags", "paths"].iter().any(|p| head.contains(&format!("{p}: &[&")));
            let under = DISPATCHERS.iter().find(|(f, n, _)| *f == file && *n == name).map(|(.., u)| *u);
            out.push(Fn { name, lines: i..end + 1, under, argv: argv || under.is_some() });
        }
        out
    }

    /// What one line (a wrapped arm joined) dispatches, or why the guard refuses it.
    struct Line {
        /// The command paths it reads.
        found: Vec<Vec<String>>,
        /// It is an arm: whether its body is `match <name> {`.
        arm: Option<bool>,
    }

    /// Read one line of an argv reader (`argv`) or of other code, per the rules above.
    fn read_line(text: &str, argv: bool) -> Result<Line, String> {
        let lits: Vec<(usize, &str)> = literals(text).into_iter().filter(|(_, c)| word(c)).collect();
        let arw = arrow(text);
        let mut consumed: Vec<std::ops::Range<usize>> = vec![];
        let mut line = Line { found: vec![], arm: None };
        let words_in = |r: std::ops::Range<usize>| lits.iter().any(|(q, _)| r.contains(q));
        if let (Some(a), true) = (arw, text.starts_with('[')) {
            let s = slice_paths(text)?;
            if argv && s.stray {
                return Err("a literal word after a binding in a slice pattern".into());
            }
            if argv && words_in(s.end..a) {
                return Err("a literal word in an arm's guard".into());
            }
            consumed.push(0..s.end);
            line = Line { found: s.paths, arm: Some(text.trim_end().ends_with('{') && text[a + 2..].trim_start().starts_with("match ")) };
        } else if let (Some(a), true) = (arw, argv && text.starts_with('"')) {
            let head = text[..a].split(" if ").next().unwrap_or("").trim();
            let alts: Vec<&str> = head.split('|').map(str::trim).collect();
            if !alts.iter().all(|a| a.len() > 2 && a.starts_with('"') && a.ends_with('"')) {
                return Err("a string arm the guard cannot read".into());
            }
            if argv && words_in(head.len()..a) {
                return Err("a literal word in an arm's guard".into());
            }
            consumed.push(0..head.len());
            let found = alts.iter().map(|a| a.trim_matches('"')).filter(|a| word(a)).map(|a| vec![a.to_string()]).collect();
            line = Line { found, arm: Some(text.trim_end().ends_with('{') && text[a + 2..].trim_start().starts_with("match ")) };
        } else if arw.is_some_and(|a| argv && words_in(0..a)) {
            return Err("an arm the guard cannot read holding a literal word".into());
        } else if let Some(i) = text.find("let [").map(|i| i + 4).or_else(|| text.find("matches!(").and_then(|i| text[i..].find('[').map(|j| i + j))) {
            let s = slice_paths(&text[i..])?;
            if argv && s.stray {
                return Err("a literal word after a binding in a slice pattern".into());
            }
            consumed.push(i..i + s.end);
            line.found = s.paths;
        } else if let Some(i) = text.find(".first() == Some(&\"") {
            let (q, w) = literals(&text[i..])[0];
            consumed.push(i + q..i + q + w.len() + 2);
            line.found = vec![vec![w.to_string()]].into_iter().filter(|_| word(w)).collect();
        }
        if !argv {
            return Ok(line);
        }
        // Default refusal: every other literal word in an argv reader is a plain value, or refused.
        let in_let = text.find("let ").map(|i| i..text[i..].find(" = ").map_or(text.len(), |e| i + e));
        let in_matches = text.find("matches!(").map(|i| i..text.len());
        for (q, w) in &lits {
            if consumed.iter().any(|r| r.contains(q)) {
                continue;
            }
            if in_let.as_ref().is_some_and(|r| r.contains(q)) {
                return Err("a literal word in a `let` pattern the guard cannot read".into());
            }
            if in_matches.as_ref().is_some_and(|r| r.contains(q)) {
                return Err("a literal word in a `matches!` the guard cannot read".into());
            }
            if compared(text, *q, w.len()) {
                return Err("a literal word compared".into());
            }
        }
        Ok(line)
    }

    /// Every command path keel dispatches, or every line the guard refuses (see the comment above).
    fn dispatched() -> Result<Vec<Vec<String>>, Vec<String>> {
        let mut problems = vec![];
        for (file, src) in SOURCES {
            let code = &src[..src.find("#[cfg(test)]\nmod tests").unwrap_or(src.len())];
            for m in
                code.lines().map(str::trim).filter_map(|l| ["pub(crate) ", "pub "].iter().fold(l, |l, p| l.strip_prefix(p).unwrap_or(l)).strip_prefix("mod "))
            {
                if let Some(m) = m.strip_suffix(';').filter(|m| !SOURCES.iter().any(|(f, _)| *f == format!("{m}.rs"))) {
                    problems.push(format!("{file}: module {m} is not read by this guard: add it to SOURCES"));
                }
            }
        }
        let mut paths = vec![];
        for (file, src) in SOURCES {
            let code = &src[..src.find("#[cfg(test)]\nmod tests").unwrap_or(src.len())];
            let lines: Vec<&str> = code.lines().collect();
            let fns = fns(file, &lines);
            for want in DISPATCHERS.iter().filter(|(f, ..)| f == file).map(|(_, n, _)| n) {
                if !fns.iter().any(|f| f.name == *want && f.under.is_some()) {
                    problems.push(format!("{file}: dispatcher {want} not found"));
                }
            }
            // Arms seen in the current fn: (indent, its words, whether its body is `match <name> {`).
            let mut arms: Vec<(usize, Vec<Vec<String>>, bool)> = vec![];
            let mut skip = 0;
            for (n, line) in lines.iter().enumerate() {
                let at = format!("{file}:{}", n + 1);
                let f = fns.iter().filter(|f| f.lines.contains(&n)).min_by_key(|f| f.lines.len());
                if f.is_some_and(|f| f.lines.start == n) {
                    arms.clear();
                }
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let t = line.trim_start();
                if t.starts_with("//") || NOT_COMMANDS.iter().any(|(nf, text, _)| nf == file && t.contains(text)) {
                    continue;
                }
                let indent = line.len() - t.len();
                let (argv, under) = (f.is_some_and(|f| f.argv), f.and_then(|f| f.under));
                let words = literals(t).into_iter().any(|(_, c)| word(c));
                let mut refuse = |why: &str| problems.push(format!("unreadable dispatch at {at}: {why}: {}", t.trim_end()));
                if words && ["Some([", "Ok([", "Err([", "@ [", "@ ("].iter().any(|s| t.contains(s)) {
                    refuse("a pattern shape the guard does not read");
                    continue;
                }
                // A slice arm may wrap over `|` lines: join it.
                let mut text = t.strip_prefix("| ").filter(|r| r.starts_with('[')).unwrap_or(t).to_string();
                if text.starts_with('[') && arrow(&text).is_none() {
                    let mut k = n;
                    while arrow(&text).is_none() && lines.get(k + 1).is_some_and(|l| ["|", "=>", "if "].iter().any(|p| l.trim_start().starts_with(p))) {
                        k += 1;
                        text = format!("{text} {}", lines[k].trim());
                    }
                    if arrow(&text).is_some() {
                        skip = k - n;
                    } else if argv && !t.contains(']') {
                        refuse("a slice pattern with no `]` on its line");
                        continue;
                    } else {
                        text = t.to_string();
                    }
                }
                let read = match read_line(&text, argv) {
                    Ok(read) => read,
                    Err(why) if argv || words => {
                        refuse(&why);
                        continue;
                    }
                    Err(_) => continue,
                };
                // Place it: at the dispatcher's root, or under the arm whose `match` it sits in.
                let parent = arms.iter().rev().find(|(i, ..)| *i < indent).cloned();
                if let Some(opens) = read.arm {
                    arms.retain(|(i, ..)| *i < indent);
                    arms.push((indent, read.found.clone(), opens));
                }
                if read.found.is_empty() {
                    continue;
                }
                let Some(under) = under else {
                    refuse("a dispatch outside every dispatcher in DISPATCHERS");
                    continue;
                };
                let base: Vec<Vec<String>> = match parent {
                    None => vec![vec![]],
                    Some((_, words, true)) => words,
                    Some(_) => {
                        refuse("a dispatch nested in an arm other than `=> match <name> {`");
                        continue;
                    }
                };
                for b in &base {
                    for p in &read.found {
                        paths.push([under.iter().map(|w| w.to_string()).collect(), b.clone(), p.clone()].concat());
                    }
                }
            }
        }
        if problems.is_empty() {
            paths.sort();
            paths.dedup();
            Ok(paths)
        } else {
            Err(problems)
        }
    }

    /// Review rounds 1-3, I1: every command a dispatcher matches has its own TABLE row - never its
    /// parent's usage, whatever that usage names - and every row leads to a dispatched command.
    #[test]
    fn every_dispatched_command_has_its_own_usage() {
        let paths = dispatched().unwrap_or_else(|p| panic!("the guard cannot read every dispatch:\n{}", p.join("\n")));
        for want in
            [&["mr", "confirm"][..], &["mr"], &["notes", "write", "cr"], &["hook", "pre-push"], &["verify-dod", "range"], &["hooks", "install"], &["init"]]
        {
            assert!(paths.iter().any(|p| p == want), "the source reader missed {want:?}: {paths:?}");
        }
        let rows: Vec<Vec<String>> = TABLE.iter().map(|(k, _)| k.iter().map(|w| w.to_string()).collect()).collect();
        let missing: Vec<&Vec<String>> = paths.iter().filter(|p| !rows.contains(p)).collect();
        assert!(missing.is_empty(), "dispatched with no usage row of their own: {missing:?}");
        let dead: Vec<&Vec<String>> = rows.iter().filter(|r| !r.is_empty() && !paths.iter().any(|p| p.starts_with(r))).collect();
        assert!(dead.is_empty(), "usage rows for no dispatched command: {dead:?}");
    }

    #[test]
    fn the_slice_reader_reads_alternatives_stops_at_a_flag_or_binding_and_refuses_what_it_cannot_read() {
        let p = |t: &str| slice_paths(t).map(|s| s.paths);
        let w = |v: &[&[&str]]| Ok(v.iter().map(|p| p.iter().map(|w| w.to_string()).collect()).collect::<Vec<Vec<String>>>());
        assert_eq!(p(r#"["hooks", "install", "--force"] => x"#), w(&[&["hooks", "install"]]));
        assert_eq!(p(r#"["init", flags @ ..] => match flags {"#), w(&[&["init"]]));
        assert_eq!(p(r#"["--force"] | ["-f"] => y"#), w(&[]));
        assert_eq!(p(r#"["lint"] | ["doctor", path] => z"#), w(&[&["lint"], &["doctor"]]));
        assert_eq!(p(r#"["a" | "b", "c", rest @ ..] = args"#), w(&[&["a", "c"], &["b", "c"]]));
        assert_eq!(p(r#"[lvl, tip, rest @ ..] => {"#), w(&[]));
        let s = slice_paths(r#"[x, "w"] if y => z"#).unwrap();
        assert!(s.stray && s.paths.is_empty() && &r#"[x, "w"] if y => z"#[s.end..] == "if y => z");
        for bad in [r#"[DOCTOR, ..] =>"#, r#"["a", ("b", c)] =>"#, r#"["a", ["b"]] =>"#, r#"["a","#] {
            assert!(slice_paths(bad).is_err(), "{bad}");
        }
        assert_eq!(literals(r#"x == Some(&"a\"b") || "c""#), [(11, r#"a\"b"#), (22, "c")]);
        assert!(word("mr") && word("verify-dod") && !word("--target") && !word("") && !word("a b"));
    }

    /// Review round 4: in an argv reader a literal word is read, a plain value, or refused.
    #[test]
    fn in_an_argv_reader_a_literal_word_is_read_a_plain_value_or_refused() {
        let found = |t: &str| read_line(t, true).map(|l| l.found);
        assert_eq!(found(r#"["mr", rest @ ..] => mr::run(&repo, rest),"#), Ok(vec![vec!["mr".to_string()]]));
        assert_eq!(found(r#"if args.first() == Some(&"mr") {"#), Ok(vec![vec!["mr".to_string()]]));
        assert_eq!(found(r#""doc" => (DOC_FLAGS, DOC_USAGE),"#), Ok(vec![vec!["doc".to_string()]]));
        for value in
            [r#"let tip = git(repo, &["rev-parse", "HEAD"], &[])?;"#, r#"[l] => (l, "HEAD"),"#, r#"let receipt = opts("verify-dod", tip, rest, true)?;"#]
        {
            assert_eq!(found(value), Ok(vec![]), "a plain value: {value}");
        }
        for refused in [
            r#"[w, ..] if *w == "tip" => Ok(0),"#,
            r#"[w, ..] if w == &"tip" => Ok(0),"#,
            r#"if let Some("tip") = args.first().copied() {"#,
            r#"Some("tip") => Ok(0),"#,
            r#"Ok("tip") => Ok(0),"#,
            r#"(Some("tip"), _) => Ok(0),"#,
            r#"[x, "tip"] => Ok(0),"#,
            r#"if args.get(1) == Some(&"tip") {"#,
            r#"if "tip" == args[0] {"#,
            r#"if args.contains(&"tip") {"#,
            r#"if args[0].starts_with("tip") {"#,
            r#"if matches!(args.first(), Some(&"tip")) {"#,
            r#"let Some("tip") = args.first().copied() else { return Ok(1) };"#,
            r#""tip" if *x == "fix" => Ok(0),"#,
        ] {
            assert!(read_line(refused, true).is_err(), "not refused: {refused}");
            assert!(read_line(refused, false).is_ok(), "outside an argv reader only slice dispatch is read: {refused}");
        }
    }
}
