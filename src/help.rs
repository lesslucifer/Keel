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
    // `#[cfg(test)]` fn is read). Refused anywhere in that code:
    //   - a declared module the guard does not read;
    //   - a slice pattern holding a literal word (`[..] =>` arms, wrapped `| [..] =>` lines,
    //     `let [`, `if let [`, `matches!(.., [..])`) outside every dispatcher in DISPATCHERS;
    //   - `Some([`, `Ok([`, `Err([`, `@ [` or `@ (` on a line holding a literal word.
    // Refused in an argv reader - a dispatcher, or any fn with an `args`, `rest`, `flags` or `paths`
    // parameter of type `&[&str]` - unless read below: a literal word compared (`==`, `!=`,
    // `Some(&"..")`), matched in a string or tuple arm, or a pattern line it cannot parse (a const,
    // a tuple or nested slice element, a `[` with no `]`).
    // Read, in a dispatcher: slice-pattern arms and `let`/`matches!` patterns, `args.first() ==
    // Some(&"w")`, and string arms (`"w" =>`, `"a" | "b" =>`, `"w" if .. =>`). A pattern is placed
    // under the dispatcher's words, or under its enclosing arm's words when that arm's body is
    // `match <name> {`; an arm nested any other way is refused. A literal word is a string literal
    // that starts with a letter or digit (a flag is no command). NOT_COMMANDS lists the reviewed
    // exceptions. Known limit: a word reaching a dispatch through a variable or a function that
    // is not an argv reader (e.g. `gate::level_of(s)`, whose levels are values, not commands).

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
    ];

    /// The string literals on a line, as written between their quotes.
    fn literals(line: &str) -> Vec<&str> {
        let (mut out, mut start, mut esc) = (vec![], None, false);
        for (i, c) in line.char_indices() {
            match (start, c) {
                (Some(_), '\\') if !esc => esc = true,
                (Some(s), '"') if !esc => {
                    out.push(&line[s..i]);
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

    /// The leading literal words of each slice pattern at the start of `t` (`[..] | [..]`
    /// alternatives and `"a" | "b"` elements included); a flag, a binding or `..` ends a path.
    /// `Err` for an element it cannot read: a const, a tuple or a nested slice, or no `]`.
    fn slice_paths(mut t: &str) -> Result<Vec<Vec<String>>, String> {
        let mut out = vec![];
        while let Some(rest) = t.strip_prefix('[') {
            let end = rest.find(']').ok_or("a slice pattern with no `]` on its line")?;
            let mut paths = vec![vec![]];
            let mut open = true;
            for e in rest[..end].split(',').map(str::trim).filter(|e| !e.is_empty()) {
                let alts: Vec<&str> = e.split('|').map(str::trim).collect();
                let lit = |a: &&str| a.len() > 2 && a.starts_with('"') && a.ends_with('"');
                if alts.iter().any(|a| a.starts_with(['(', '[']) || a.chars().next().is_some_and(|c| c.is_ascii_uppercase())) {
                    return Err(format!("an element it cannot read: {e}"));
                }
                if !open || !alts.iter().all(lit) || alts.iter().any(|a| a.starts_with("\"-")) {
                    open = false;
                    continue;
                }
                paths =
                    paths.iter().flat_map(|p: &Vec<String>| alts.iter().map(move |a| [p.clone(), vec![a.trim_matches('"').to_string()]].concat())).collect();
            }
            out.extend(paths.into_iter().filter(|p| !p.is_empty()));
            t = rest[end..].trim_start_matches(']').trim_start();
            t = t.strip_prefix('|').map(str::trim_start).unwrap_or("");
        }
        Ok(out)
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
            for (want, ..) in DISPATCHERS.iter().filter(|(f, ..)| f == file).map(|(_, n, u)| (n, u)) {
                if !fns.iter().any(|f| f.name == **want && f.under.is_some()) {
                    problems.push(format!("{file}: dispatcher {want} not found"));
                }
            }
            // Arms seen in the current dispatcher: (indent, its words, whether its body is `match <name> {`).
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
                if t.starts_with("//") {
                    continue;
                }
                let indent = line.len() - t.len();
                let words = literals(t).into_iter().any(word);
                let excused = NOT_COMMANDS.iter().any(|(nf, text, _)| nf == file && t.contains(text));
                let (argv, under) = (f.is_some_and(|f| f.argv), f.and_then(|f| f.under));
                let mut refuse = |why: &str| problems.push(format!("unreadable dispatch at {at}: {why}: {}", t.trim_end()));
                if excused {
                    continue;
                }
                if words && ["Some([", "Ok([", "Err([", "@ [", "@ ("].iter().any(|s| t.contains(s)) {
                    refuse("a pattern shape the guard does not read");
                    continue;
                }
                // A pattern: an arm (possibly wrapped over `|` lines), a `let`, or a `matches!`.
                let mut pat: Option<(String, bool)> = None;
                if t.starts_with('[') || t.starts_with("| [") {
                    let mut joined = t.trim_start_matches("| ").to_string();
                    let mut k = n;
                    while !joined.contains("=>") && lines.get(k + 1).is_some_and(|l| ["|", "=>", "if "].iter().any(|p| l.trim_start().starts_with(p))) {
                        k += 1;
                        joined = format!("{joined} {}", lines[k].trim());
                    }
                    if joined.contains("=>") {
                        skip = k - n;
                        pat = Some((joined, true));
                    } else if argv && !t.contains(']') {
                        refuse("a slice pattern with no `]` on its line");
                        continue;
                    }
                } else if let Some(i) = t.find("let [") {
                    pat = Some((t[i + 4..].to_string(), false));
                } else if let Some(i) = t.find("matches!(") {
                    if let Some(j) = t[i..].find('[') {
                        pat = Some((t[i + j..].to_string(), false));
                    } else if argv && words {
                        refuse("a matches! the guard does not read");
                        continue;
                    }
                }
                let scalar_arm = t.starts_with('"') && t.contains("=>");
                let tuple_arm = t.starts_with('(') && t.contains("=>") && words;
                let first = t.find(".first() == Some(&\"").map(|i| literals(&t[i..])[0].to_string());
                let compared = words && (t.contains("==") || t.contains("!=") || t.contains("Some(&\""));
                let found: Vec<Vec<String>> = if let Some((p, arm)) = &pat {
                    match slice_paths(p) {
                        Err(why) if argv || words => {
                            refuse(&why);
                            continue;
                        }
                        Err(_) => continue,
                        Ok(found) => {
                            if *arm {
                                let opens = p.trim_end().ends_with('{') && p.split("=>").nth(1).is_some_and(|b| b.trim().starts_with("match "));
                                arms.retain(|(i, ..)| *i < indent);
                                arms.push((indent, found.clone(), opens));
                            }
                            found
                        }
                    }
                } else if scalar_arm && argv {
                    let head = t[..t.find("=>").unwrap()].split(" if ").next().unwrap().trim();
                    let alts: Vec<&str> = head.split('|').map(str::trim).collect();
                    if !alts.iter().all(|a| a.len() > 2 && a.starts_with('"') && a.ends_with('"')) {
                        refuse("a string arm the guard cannot read");
                        continue;
                    }
                    let found: Vec<Vec<String>> = alts.iter().map(|a| a.trim_matches('"')).filter(|a| word(a)).map(|a| vec![a.to_string()]).collect();
                    arms.retain(|(i, ..)| *i < indent);
                    arms.push((indent, found.clone(), t.trim_end().ends_with('{') && t.contains("=> match ")));
                    found
                } else if tuple_arm && argv {
                    refuse("a tuple arm holding a literal word");
                    continue;
                } else if let Some(w) = first.filter(|w| word(w)) {
                    vec![vec![w]]
                } else if compared && argv {
                    refuse("a literal word compared");
                    continue;
                } else {
                    continue;
                };
                if found.is_empty() {
                    continue;
                }
                let Some(under) = under else {
                    refuse("a dispatch outside every dispatcher in DISPATCHERS");
                    continue;
                };
                // Place it: at the dispatcher's root, or under the arm whose `match` it sits in.
                let parent = arms.iter().rev().find(|(i, ..)| *i < indent);
                let base: Vec<Vec<String>> = match parent {
                    None => vec![vec![]],
                    Some((_, words, true)) => words.clone(),
                    Some(_) => {
                        refuse("a dispatch nested in an arm other than `=> match <name> {`");
                        continue;
                    }
                };
                for b in &base {
                    for p in &found {
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
        let p = |v: &[&[&str]]| Ok(v.iter().map(|p| p.iter().map(|w| w.to_string()).collect()).collect::<Vec<Vec<String>>>());
        assert_eq!(slice_paths(r#"["hooks", "install", "--force"] => x"#), p(&[&["hooks", "install"]]));
        assert_eq!(slice_paths(r#"["init", flags @ ..] => match flags {"#), p(&[&["init"]]));
        assert_eq!(slice_paths(r#"["--force"] | ["-f"] => y"#), p(&[]));
        assert_eq!(slice_paths(r#"["lint"] | ["doctor", path] => z"#), p(&[&["lint"], &["doctor"]]));
        assert_eq!(slice_paths(r#"["a" | "b", "c", rest @ ..] = args"#), p(&[&["a", "c"], &["b", "c"]]));
        assert_eq!(slice_paths(r#"[lvl, tip, rest @ ..] => {"#), p(&[]));
        for bad in [r#"[DOCTOR, ..] =>"#, r#"["a", ("b", c)] =>"#, r#"["a", ["b"]] =>"#, r#"["a","#] {
            assert!(slice_paths(bad).is_err(), "{bad}");
        }
        assert_eq!(literals(r#"x == Some(&"a\"b") || "c""#), [r#"a\"b"#, "c"]);
        assert!(word("mr") && word("verify-dod") && !word("--target") && !word("") && !word("a b"));
    }
}
