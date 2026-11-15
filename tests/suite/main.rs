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

mod cli;
mod close_walk;
mod fixtures;
mod init;
mod layout;
mod mr;
mod release;
mod review;
mod router;
mod seed3_e2e;
mod verify_doc;
mod verify_dod;
