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
/// is read from the other arguments' leading words, up to the first that starts with `-`.
pub(crate) fn usage_for(args: &[&str]) -> Option<&'static str> {
    if !args.iter().any(|a| is_help(a)) {
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
        (&["hook", "commit-msg", ".git/COMMIT_EDITMSG"], super::HOOK),
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
}
