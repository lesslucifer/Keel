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
const TABLE: &[(&[&str], &str)] = &[
    (&[], TOP),
    (&["init"], INIT),
    (&["hooks"], HOOKS),
    (&["lint"], LINT),
    (&["check"], CHECK),
    (&["hook"], HOOK),
    (&["verify-doc"], VERIFY_DOC),
    (&["verify-dod"], gate::USAGE),
    (&["preflight"], gate::USAGE),
    (&["notes"], notes::NOTES_USAGE),
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

    /// The command paths each dispatcher matches, read from its source: `run()`'s slice patterns,
    /// `gate::verify_dod`'s (under `verify-dod`), `mr::run`'s (under `mr`) and `notes::write`'s kinds
    /// (under `notes write`). Flags and bindings end a path.
    fn dispatched() -> Vec<Vec<String>> {
        fn body<'a>(src: &'a str, sig: &str) -> &'a str {
            let at = src.find(sig).unwrap_or_else(|| panic!("no {sig}"));
            &src[at..at + src[at..].find("\n}\n").unwrap()]
        }
        let slices = |body: &str, under: &[&str]| -> Vec<Vec<String>> {
            body.lines()
                .filter_map(|l| l.trim().strip_prefix('[').filter(|_| l.contains("=>")))
                .map(|l| {
                    let words = l.split(['[', ']', ',', ' ', '|']).take_while(|w| w.is_empty() || (w.starts_with('"') && !w.starts_with("\"-")));
                    under.iter().map(|w| w.to_string()).chain(words.filter(|w| !w.is_empty()).map(|w| w.trim_matches('"').to_string())).collect()
                })
                .filter(|p: &Vec<String>| p.len() > under.len())
                .collect()
        };
        let mut all = slices(body(include_str!("main.rs"), "fn run() ->"), &[]);
        all.extend(slices(body(include_str!("gate.rs"), "pub fn verify_dod("), &["verify-dod"]));
        all.extend(slices(body(include_str!("mr.rs"), "pub fn run("), &["mr"]));
        let kinds = body(include_str!("notes.rs"), "pub fn write(")
            .lines()
            .filter_map(|l| l.trim().split_once("\" =>").map(|(k, _)| k.trim_start_matches('"').to_string()));
        all.extend(kinds.map(|k| vec!["notes".into(), "write".into(), k]));
        all
    }

    /// Round 1, I1: a command a dispatcher matches has its own usage - its own row, or its parent's
    /// usage naming it - so a new subcommand never prints its parent's text in silence.
    #[test]
    fn every_dispatched_command_has_its_own_usage() {
        let paths = dispatched();
        for want in [&["mr", "confirm"][..], &["notes", "write", "cr"], &["hook", "pre-push"], &["verify-dod", "range"], &["hooks", "install"], &["init"]] {
            assert!(paths.iter().any(|p| p == want), "the source reader missed {want:?}: {paths:?}");
        }
        for path in paths {
            let words: Vec<&str> = path.iter().map(String::as_str).collect();
            let (key, usage) = TABLE.iter().filter(|(k, _)| words.starts_with(k)).max_by_key(|(k, _)| k.len()).unwrap();
            let named = |w: &str| usage.split(|c: char| !(c.is_alphanumeric() || c == '-')).any(|t| t == w);
            assert!(words[key.len()..].iter().all(|w| named(w)), "{words:?} gets the usage of {key:?}, which does not name it: {usage}");
        }
    }
}
