//! Keel's one integration test program (E-TESTOPT.W3): every test file is a module here, so
//! `cargo test` runs them in one pool and never stops at the first failing file.
//! `layout::the_suite_is_the_only_test_program` keeps it that way.

/// Every file of the suite calls this once. Its test prints the file rustc compiled it from, so
/// a file whose content never reached the built suite, for whatever reason, prints nothing and
/// `layout::the_suite_is_the_only_test_program` names it.
macro_rules! compiled {
    () => {
        #[test]
        fn compiled_file() {
            println!("{}{}", $crate::layout::MARK, file!());
        }
    };
}

compiled!();

#[path = "../common/mod.rs"]
mod common;

mod ci_gate;
mod cli;
mod close_walk;
mod fixtures;
mod ghflow_e2e;
mod init;
mod keel_gate;
mod layout;
mod mr;
mod notify_owner;
mod pages_build;
mod pages_workflow;
mod release;
mod review;
mod router;
mod router_gh;
mod router_gh_args;
mod router_gh_watch;
mod router_gh_watch_args;
mod router_inbox;
mod seed3_e2e;
mod settings_check;
mod sign_off;
mod verify_doc;
mod verify_dod;
