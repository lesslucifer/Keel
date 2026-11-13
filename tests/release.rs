//! The release contract (DEC-38): the licence npm and Homebrew carry, prereleases never
//! reaching npm or Homebrew, and the hardened dist config `release.yml` is generated from.
use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

fn toml_file(rel: &str) -> toml::Table {
    read(rel).parse().unwrap()
}

fn dist() -> toml::Table {
    toml_file("dist-workspace.toml")["dist"].as_table().unwrap().clone()
}

fn job(yml: &str, name: &str) -> String {
    let header = format!("  {name}:");
    let mut lines = yml.lines().skip_while(|l| *l != header);
    assert!(lines.next().is_some(), "job {name} missing");
    lines.take_while(|l| l.is_empty() || l.starts_with("   ")).collect::<Vec<_>>().join("\n")
}

#[test]
fn licence_is_mit_and_dist_ships_it() {
    assert_eq!(toml_file("Cargo.toml")["package"]["license"].as_str(), Some("MIT"));
    let licence = read("LICENSE");
    assert!(licence.starts_with("MIT License\n\nCopyright (c) "));
    assert!(licence.contains("Permission is hereby granted, free of charge"));
    assert!(licence.contains("THE SOFTWARE IS PROVIDED \"AS IS\""));
    assert_ne!(dist().get("auto-includes").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn job_block_ends_at_the_next_job() {
    let yml = "jobs:\n  a:\n    if: x\n\n    steps: y\n  b:\n    if: z\n";
    assert_eq!(job(yml, "a"), "    if: x\n\n    steps: y");
    assert_eq!(job(yml, "b"), "    if: z");
}

#[test]
fn prerelease_tags_never_publish_to_npm_or_homebrew() {
    assert_ne!(dist().get("publish-prereleases").and_then(|v| v.as_bool()), Some(true));
    let yml = read(".github/workflows/release.yml");
    let guard = "if: ${{ !fromJson(needs.plan.outputs.val).announcement_is_prerelease || fromJson(needs.plan.outputs.val).publish_prereleases }}";
    for name in ["publish-npm", "publish-homebrew-formula"] {
        assert!(job(&yml, name).contains(guard), "{name} lost its prerelease guard");
    }
}

#[test]
fn dist_pins_every_action_it_uses_to_a_commit() {
    let d = dist();
    let pins = d["github-action-commits"].as_table().unwrap();
    for action in ["actions/checkout", "actions/upload-artifact", "actions/download-artifact", "actions/setup-node"] {
        let sha = pins.get(action).and_then(|v| v.as_str()).unwrap_or_else(|| panic!("{action} not pinned"));
        assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{action} = {sha}");
    }
}

#[test]
fn dist_runs_linux_jobs_on_ubuntu_24_04() {
    let d = dist();
    let runners = d["github-custom-runners"].as_table().unwrap();
    for key in ["global", "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"] {
        let runner = runners.get(key).and_then(|v| v.as_str()).unwrap_or_else(|| panic!("{key} has no runner"));
        assert!(runner.starts_with("ubuntu-24.04"), "{key} = {runner}");
    }
}
