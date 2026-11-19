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
/// Every command a dispatcher matches has its own row here (the test
/// `every_dispatched_command_has_its_own_usage` reads every dispatcher and holds this to it).
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

    /// Every module `main.rs` declares, as compiled, so the guard below reads the whole program.
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

    /// Where keel dispatches a command: the function, and the words that lead to it.
    const DISPATCHERS: &[(&str, &str, &[&str])] = &[
        ("main.rs", "fn run() ->", &[]),
        ("gate.rs", "pub fn verify_dod(", &["verify-dod"]),
        ("gate.rs", "pub fn preflight_cmd(", &["preflight"]),
        ("mr.rs", "pub fn run(", &["mr"]),
        ("notes.rs", "pub fn write(", &["notes", "write"]),
    ];

    /// Slice patterns that lead with a literal word yet dispatch no command, and why.
    const NOT_COMMANDS: &[(&str, &str)] = &[("mr.rs", r#"if let ["100644" | "100755", "blob", id]"#)]; // a `git ls-tree` line

    /// The leading literal words of each slice pattern at the start of `t` (`[..] | [..]` alternatives
    /// and `"a" | "b"` elements included); a flag, a binding or `..` ends a path.
    fn slice_paths(mut t: &str) -> Vec<Vec<String>> {
        let mut out = vec![];
        while let Some(rest) = t.strip_prefix('[') {
            let end = rest.find(']').unwrap_or(rest.len());
            let mut paths = vec![vec![]];
            for e in rest[..end].split(',') {
                let alts: Vec<&str> = e.split('|').map(str::trim).collect();
                if !alts.iter().all(|a| a.len() > 2 && a.starts_with('"') && a.ends_with('"') && !a.starts_with("\"-")) {
                    break;
                }
                paths =
                    paths.iter().flat_map(|p: &Vec<String>| alts.iter().map(move |a| [p.clone(), vec![a.trim_matches('"').to_string()]].concat())).collect();
            }
            out.extend(paths.into_iter().filter(|p| !p.is_empty()));
            t = rest[end..].trim_start_matches(']').trim_start();
            t = t.strip_prefix('|').map(str::trim_start).unwrap_or("");
        }
        out
    }

    /// Every command path keel dispatches: each slice pattern leading with a literal word, in any
    /// module (`match` arms and `let` patterns alike), placed under the dispatcher it sits in, plus
    /// `notes::write`'s kinds. A pattern outside every dispatcher, or a module the guard does not
    /// read, is an error: the guard never passes a dispatcher it cannot see.
    fn dispatched() -> Result<Vec<Vec<String>>, Vec<String>> {
        let main = SOURCES[0].1;
        let mut problems: Vec<String> = main
            .lines()
            .filter_map(|l| l.strip_prefix("mod ").and_then(|m| m.strip_suffix(';')))
            .filter(|m| !SOURCES.iter().any(|(f, _)| *f == format!("{m}.rs")))
            .map(|m| format!("module {m} is not read by this guard: add it to SOURCES"))
            .collect();
        let mut paths = vec![];
        for (file, src) in SOURCES {
            let code = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
            let bodies: Vec<(std::ops::Range<usize>, &[&str])> = DISPATCHERS
                .iter()
                .filter(|(f, ..)| f == file)
                .map(|(_, sig, under)| {
                    let at = code.find(sig).unwrap_or_else(|| panic!("{file}: no {sig}"));
                    (at..at + code[at..].find("\n}\n").unwrap(), *under)
                })
                .collect();
            let mut offset = 0;
            for line in code.split_inclusive('\n') {
                let at = offset;
                offset += line.len();
                let t = line.trim_start();
                let pat = if t.starts_with('[') && t.contains("=>") { Some(t) } else { t.find("let [").map(|i| &t[i + 4..]) };
                let found = pat.map(slice_paths).unwrap_or_default();
                if found.is_empty() || NOT_COMMANDS.iter().any(|(f, text)| f == file && t.contains(text)) {
                    continue;
                }
                match bodies.iter().find(|(r, _)| r.contains(&at)) {
                    Some((_, under)) => paths.extend(found.into_iter().map(|p| [under.iter().map(|w| w.to_string()).collect(), p].concat())),
                    None => problems.push(format!("{file}: a pattern outside every dispatcher the guard reads: {}", t.trim_end())),
                }
            }
            if *file == "notes.rs" {
                let (r, _) = &bodies[0];
                let kinds = code[r.clone()].lines().filter_map(|l| l.trim().split_once("\" =>").map(|(k, _)| k.trim_start_matches('"').to_string()));
                paths.extend(kinds.map(|k| vec!["notes".into(), "write".into(), k]));
            }
        }
        if problems.is_empty() {
            Ok(paths)
        } else {
            Err(problems)
        }
    }

    /// Review rounds 1 and 2, I1: every command a dispatcher matches has its own TABLE row - never
    /// its parent's usage, whatever that usage names - and every row leads to a dispatched command.
    #[test]
    fn every_dispatched_command_has_its_own_usage() {
        let paths = dispatched().unwrap_or_else(|p| panic!("the guard cannot read every dispatcher:\n{}", p.join("\n")));
        for want in [&["mr", "confirm"][..], &["notes", "write", "cr"], &["hook", "pre-push"], &["verify-dod", "range"], &["hooks", "install"], &["init"]] {
            assert!(paths.iter().any(|p| p == want), "the source reader missed {want:?}: {paths:?}");
        }
        let rows: Vec<Vec<String>> = TABLE.iter().map(|(k, _)| k.iter().map(|w| w.to_string()).collect()).collect();
        let missing: Vec<&Vec<String>> = paths.iter().filter(|p| !rows.contains(p)).collect();
        assert!(missing.is_empty(), "dispatched with no usage row of their own: {missing:?}");
        let dead: Vec<&Vec<String>> = rows.iter().filter(|r| !r.is_empty() && !paths.iter().any(|p| p.starts_with(r))).collect();
        assert!(dead.is_empty(), "usage rows for no dispatched command: {dead:?}");
    }

    #[test]
    fn the_slice_reader_reads_alternatives_and_stops_at_a_flag_or_binding() {
        let p = |v: &[&[&str]]| v.iter().map(|p| p.iter().map(|w| w.to_string()).collect()).collect::<Vec<Vec<String>>>();
        assert_eq!(slice_paths(r#"["hooks", "install", "--force"] => x"#), p(&[&["hooks", "install"]]));
        assert_eq!(slice_paths(r#"["init", flags @ ..] => match flags {"#), p(&[&["init"]]));
        assert_eq!(slice_paths(r#"["--force"] | ["-f"] => y"#), p(&[]));
        assert_eq!(slice_paths(r#"["lint"] | ["doctor", path] => z"#), p(&[&["lint"], &["doctor"]]));
        assert_eq!(slice_paths(r#"["a" | "b", "c", rest @ ..] = args"#), p(&[&["a", "c"], &["b", "c"]]));
        assert_eq!(slice_paths(r#"[lvl, tip, rest @ ..] => {"#), p(&[]));
    }
}
