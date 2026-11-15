//! Keel's one integration test program (E-TESTOPT.W3): every test file is a module here, so
//! `cargo test` runs them in one pool and never stops at the first failing file.
//! `layout::the_suite_is_the_only_test_program` keeps it that way.
#[path = "../common/mod.rs"]
mod common;

mod cli;
mod close_walk;
mod init;
mod layout;
mod mr;
mod release;
mod review;
mod router;
mod seed3_e2e;
mod verify_doc;
mod verify_dod;
