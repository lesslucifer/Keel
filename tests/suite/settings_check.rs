//! E-GHFLOW2.W4: scripts/gh-settings-check.sh, the owner's read-only check of GitHub's live settings,
//! on recorded answers served by the fake curl the router's tests use (no network).
compiled!();

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

use crate::common;
use crate::router_gh::{exe, text, FAKE_CURL};

const R: &str = "/repos/own/Keel";

/// GitHub's answers: path, HTTP status, body.
type Answers = Vec<(String, u16, Value)>;

struct Gh {
    _root: TempDir,
    mock: PathBuf,
    curl: PathBuf,
}

/// GitHub's answers when every setting is as the W4 plan lists: one ruleset for main and the epics,
/// one for the tags, merge commits only, auto-merge, a read token, Pages from Actions on main.
fn green() -> Answers {
    let branch = json!({
        "id": 1, "name": "keel-merge", "target": "branch", "enforcement": "active", "bypass_actors": [],
        "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH", "refs/heads/epic/**"], "exclude": []}},
        "rules": [
            {"type": "deletion"},
            {"type": "non_fast_forward"},
            {"type": "pull_request", "parameters": {"required_approving_review_count": 0, "allowed_merge_methods": ["merge"]}},
            {"type": "required_status_checks", "parameters": {"strict_required_status_checks_policy": false, "required_status_checks": [
                {"context": "keel/ci", "integration_id": 15368},
                {"context": "keel/gate", "integration_id": 15368},
                {"context": "keel/docs", "integration_id": 15368},
                {"context": "keel/sign-off", "integration_id": 15368}
            ]}}
        ]
    });
    let tags = json!({
        "id": 2, "name": "keel-tags", "target": "tag", "enforcement": "active",
        "bypass_actors": [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}],
        "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
        "rules": [{"type": "creation"}, {"type": "update"}, {"type": "deletion"}]
    });
    vec![
        (
            R.into(),
            200,
            json!({"full_name": "own/Keel", "allow_merge_commit": true, "allow_squash_merge": false, "allow_rebase_merge": false, "allow_auto_merge": true}),
        ),
        (format!("{R}/rulesets?includes_parents=true&per_page=100"), 200, json!([{"id": 1, "name": "keel-merge"}, {"id": 2, "name": "keel-tags"}])),
        (format!("{R}/rulesets/1"), 200, branch),
        (format!("{R}/rulesets/2"), 200, tags),
        (format!("{R}/actions/permissions/workflow"), 200, json!({"default_workflow_permissions": "read", "can_approve_pull_request_reviews": false})),
        (format!("{R}/actions/permissions/fork-pr-contributor-approval"), 200, json!({"approval_policy": "all_external_contributors"})),
        (format!("{R}/pages"), 200, json!({"build_type": "workflow", "html_url": "https://own.github.io/Keel/"})),
        (
            format!("{R}/environments/github-pages"),
            200,
            json!({"name": "github-pages", "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}),
        ),
        (
            format!("{R}/environments/github-pages/deployment-branch-policies"),
            200,
            json!({"total_count": 1, "branch_policies": [{"id": 9, "name": "main", "type": "branch"}]}),
        ),
    ]
}

impl Gh {
    fn new() -> Gh {
        let root = TempDir::new().unwrap();
        let mock = root.path().join("mock");
        std::fs::create_dir_all(&mock).unwrap();
        let curl = root.path().join("fake-curl");
        exe(&curl, FAKE_CURL);
        Gh { _root: root, mock, curl }
    }

    fn serve(&self, answers: &[(String, u16, Value)]) {
        for e in std::fs::read_dir(&self.mock).unwrap() {
            std::fs::remove_file(e.unwrap().path()).unwrap();
        }
        let mut routes = String::new();
        for (i, (path, status, body)) in answers.iter().enumerate() {
            std::fs::write(self.mock.join(format!("r{i}.json")), body.to_string()).unwrap();
            routes.push_str(&format!("GET {path} {status} r{i}.json\n"));
        }
        std::fs::write(self.mock.join("routes"), routes).unwrap();
        std::fs::write(self.mock.join("log"), "").unwrap();
    }

    fn run(&self, args: &[&str], token: Option<&str>) -> Output {
        let mut c = common::cmd("sh");
        c.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/gh-settings-check.sh")).args(args).current_dir(self.mock.parent().unwrap());
        c.env("CURL", &self.curl).env("MOCK_GH", &self.mock).env_remove("API").env_remove("GH_TOKEN");
        if let Some(t) = token {
            c.env("GH_TOKEN", t);
        }
        common::hermetic(&mut c);
        c.output().unwrap()
    }

    fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.mock.join("log")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }
}

fn lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect()
}

/// The answers with `f` applied to the one for `path`.
fn edit(path: &str, f: impl Fn(&mut Value)) -> Answers {
    let mut a = green();
    let i = a.iter().position(|(p, _, _)| p == path).unwrap();
    f(&mut a[i].2);
    a
}

/// A rule of the branch ruleset, by type.
fn rule<'a>(v: &'a mut Value, ty: &str) -> &'a mut Value {
    v["rules"].as_array_mut().unwrap().iter_mut().find(|r| r["type"] == ty).unwrap()
}

#[test]
fn every_setting_as_listed_is_green_and_the_check_only_reads() {
    let gh = Gh::new();
    gh.serve(&green());
    let o = gh.run(&["own/Keel"], Some("tok-123"));
    assert!(o.status.success(), "{}", text(&o));
    let l = lines(&o);
    assert_eq!(l.last().unwrap(), "SETTINGS green (0 red, 0 unread)");
    let oks = l.iter().filter(|x| x.starts_with("OK ")).count();
    assert_eq!(oks, l.len() - 2, "every rule OK: {l:#?}");
    for id in [
        "merge.commit",
        "merge.squash",
        "merge.rebase",
        "merge.auto",
        "main.ruleset",
        "main.bypass",
        "main.pull-request",
        "main.check.keel/ci",
        "main.check.keel/gate",
        "main.check.keel/docs",
        "main.check.keel/sign-off",
        "main.no-force-push",
        "main.no-deletion",
        "epic.ruleset",
        "epic.bypass",
        "epic.check.keel/sign-off",
        "tags.ruleset",
        "tags.creation",
        "tags.update",
        "tags.deletion",
        "tags.bypass",
        "actions.token",
        "actions.approve",
        "actions.forks",
        "pages.source",
        "pages.env",
        "pages.env-branches",
    ] {
        assert!(l.iter().any(|x| x.starts_with(&format!("OK {id} "))), "{id} missing: {l:#?}");
    }
    assert!(l.iter().any(|x| x.starts_with("BY-HAND actions.pull_request_target ")), "{l:#?}");
    let log = gh.log();
    assert_eq!(log.len(), 9, "{log:#?}");
    for r in &log {
        assert_eq!(r["method"], "GET", "read-only: {r}");
        assert_eq!(r["auth"], "Bearer tok-123");
        assert_eq!(r["body"], Value::Null, "no request carries a body");
    }
    assert!(!text(&o).contains("tok-123"), "the token is never printed");
}

/// Each difference from the list is red, names what GitHub says, and fails the check.
#[test]
fn a_missing_rule_a_bypass_actor_squash_or_a_check_from_another_source_is_red() {
    let gh = Gh::new();
    let rs1 = format!("{R}/rulesets/1");
    let cases: Vec<(&str, Answers, Vec<&str>)> = vec![
        (
            "a missing rule",
            edit(&rs1, |v| v["rules"].as_array_mut().unwrap().retain(|r| r["type"] != "deletion")),
            vec!["RED main.no-deletion main cannot be deleted: allowed", "RED epic.no-deletion every epic/ branch cannot be deleted: allowed"],
        ),
        (
            "a bypass actor",
            edit(&rs1, |v| v["bypass_actors"] = json!([{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}])),
            vec![
                "RED main.bypass nobody bypasses the rules of main: RepositoryRole 5 always",
                "RED epic.bypass nobody bypasses the rules of every epic/ branch: RepositoryRole 5 always",
            ],
        ),
        ("squash allowed", edit(R, |v| v["allow_squash_merge"] = json!(true)), vec!["RED merge.squash squash merging is off: allowed"]),
        (
            "squash allowed by the ruleset",
            edit(&rs1, |v| rule(v, "pull_request")["parameters"]["allowed_merge_methods"] = json!(["merge", "squash"])),
            vec![
                "RED main.pull-request a pull request is required, merged as a merge commit only: merge methods merge+squash",
                "RED epic.pull-request a pull request is required, merged as a merge commit only: merge methods merge+squash",
            ],
        ),
        (
            "a check from another source",
            edit(&rs1, |v| rule(v, "required_status_checks")["parameters"]["required_status_checks"][0]["integration_id"] = json!(12345)),
            vec![
                "RED main.check.keel/ci keel/ci is required, from GitHub Actions: source 12345",
                "RED epic.check.keel/ci keel/ci is required, from GitHub Actions: source 12345",
            ],
        ),
        (
            "a check from any source",
            edit(&rs1, |v| {
                rule(v, "required_status_checks")["parameters"]["required_status_checks"][3].as_object_mut().unwrap().remove("integration_id");
            }),
            vec![
                "RED main.check.keel/sign-off keel/sign-off is required, from GitHub Actions: source any",
                "RED epic.check.keel/sign-off keel/sign-off is required, from GitHub Actions: source any",
            ],
        ),
        (
            "a check not required",
            edit(&rs1, |v| {
                rule(v, "required_status_checks")["parameters"]["required_status_checks"].as_array_mut().unwrap().remove(1);
            }),
            vec![
                "RED main.check.keel/gate keel/gate is required, from GitHub Actions: not required",
                "RED epic.check.keel/gate keel/gate is required, from GitHub Actions: not required",
            ],
        ),
        (
            "force push allowed",
            edit(&rs1, |v| v["rules"].as_array_mut().unwrap().retain(|r| r["type"] != "non_fast_forward")),
            vec!["RED main.no-force-push no force push to main: allowed", "RED epic.no-force-push no force push to every epic/ branch: allowed"],
        ),
        (
            "the ruleset only evaluated",
            edit(&rs1, |v| v["enforcement"] = json!("evaluate")),
            vec!["RED main.ruleset an active ruleset covers main: none", "RED epic.ruleset an active ruleset covers every epic/ branch: none"],
        ),
        (
            "the epics left out",
            edit(&rs1, |v| v["conditions"]["ref_name"]["include"] = json!(["~DEFAULT_BRANCH"])),
            vec!["RED epic.ruleset an active ruleset covers every epic/ branch: none"],
        ),
        (
            "an excluded branch",
            edit(&rs1, |v| v["conditions"]["ref_name"]["exclude"] = json!(["refs/heads/epic/hotfix"])),
            vec![
                "RED main.exclude no ruleset of main excludes a branch: refs/heads/epic/hotfix",
                "RED epic.exclude no ruleset of every epic/ branch excludes a branch: refs/heads/epic/hotfix",
            ],
        ),
        (
            "anyone may tag",
            edit(&format!("{R}/rulesets/2"), |v| v["bypass_actors"] = json!([{"actor_id": 1, "actor_type": "Integration", "bypass_mode": "always"}])),
            vec!["RED tags.bypass only a repository admin (the owner) bypasses the tag rule: Integration 1"],
        ),
        (
            "a write token",
            edit(&format!("{R}/actions/permissions/workflow"), |v| v["default_workflow_permissions"] = json!("write")),
            vec!["RED actions.token the workflow token reads by default: write"],
        ),
        (
            "Pages from a branch",
            edit(&format!("{R}/pages"), |v| v["build_type"] = json!("legacy")),
            vec!["RED pages.source Pages builds from GitHub Actions: legacy"],
        ),
    ];
    for (what, answers, want) in cases {
        gh.serve(&answers);
        let o = gh.run(&["own/Keel"], Some("t"));
        assert_eq!(o.status.code(), Some(1), "{what}: {}", text(&o));
        let l = lines(&o);
        for w in &want {
            assert!(l.iter().any(|x| x == w), "{what}: {w:?} missing in {l:#?}");
        }
        let reds = l.iter().filter(|x| x.starts_with("RED ")).count();
        assert_eq!(reds, want.len(), "{what}: only what differs is red: {l:#?}");
        assert_eq!(l.last().unwrap(), &format!("SETTINGS red ({reds} red, 0 unread)"), "{what}");
    }
}

/// Without a token GitHub hides the bypass lists and the Actions settings: those lines are UNREAD,
/// which is not green. Rules in two rulesets, one per branch kind, are as good as one.
#[test]
fn what_github_hides_is_unread_never_green_and_split_rulesets_count() {
    let gh = Gh::new();
    let mut a = green();
    for (p, s, v) in a.iter_mut() {
        if p.contains("/rulesets/") {
            v.as_object_mut().unwrap().remove("bypass_actors");
        }
        if p.contains("/actions/") {
            *s = 403;
            *v = json!({"message": "Must have admin rights to Repository."});
        }
    }
    gh.serve(&a);
    let o = gh.run(&["own/Keel"], None);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let l = lines(&o);
    for w in [
        "UNREAD main.bypass nobody bypasses the rules of main: GitHub hides the bypass list: run with GH_TOKEN",
        "UNREAD tags.bypass only a repository admin (the owner) bypasses the tag rule: GitHub hides the bypass list: run with GH_TOKEN",
        "UNREAD actions.token the workflow token reads by default: HTTP 403",
        "UNREAD actions.forks every outside contributor waits for approval: HTTP 403",
    ] {
        assert!(l.iter().any(|x| x == w), "{w:?} missing in {l:#?}");
    }
    assert_eq!(l.last().unwrap(), "SETTINGS red (0 red, 6 unread)", "three bypass lists and three Actions settings");
    assert!(gh.log().iter().all(|r| r["auth"] == ""), "no token, no Authorization header");

    // one ruleset per branch kind
    let mut a = green();
    let one = a[2].2.clone();
    let mut main = one.clone();
    main["conditions"]["ref_name"]["include"] = json!(["refs/heads/main"]);
    let mut epic = one;
    epic["id"] = json!(3);
    epic["conditions"]["ref_name"]["include"] = json!(["refs/heads/epic/*"]);
    a[1].2 = json!([{"id": 1}, {"id": 2}, {"id": 3}]);
    a[2].2 = main;
    a.push((format!("{R}/rulesets/3"), 200, epic));
    gh.serve(&a);
    let o = gh.run(&["own/Keel"], Some("t"));
    assert!(o.status.success(), "{}", text(&o));
}

/// GitHub unreachable, or Pages off, is never green; bad input is refused before any request.
#[test]
fn an_unreachable_github_pages_off_and_bad_input_are_never_green() {
    let gh = Gh::new();
    gh.serve(&[]);
    let o = gh.run(&["own/Keel"], Some("t"));
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let l = lines(&o);
    assert!(l.contains(&"RED pages.source Pages builds from GitHub Actions: Pages is off".to_string()), "{l:#?}");
    assert!(l.contains(&"UNREAD merge.commit merge commits are allowed: not found (HTTP 404)".to_string()), "{l:#?}");
    assert!(l.last().unwrap().starts_with("SETTINGS red ("), "{l:#?}");

    // curl fails on the repository's own answer (exit 7: no connection)
    gh.serve(&green());
    let routes = std::fs::read_to_string(gh.mock.join("routes")).unwrap();
    std::fs::write(gh.mock.join("routes"), routes.replacen(&format!("GET {R} 200 "), &format!("GET {R} curl7 "), 1)).unwrap();
    let o = gh.run(&["own/Keel"], Some("t"));
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(lines(&o).contains(&"UNREAD merge.auto auto-merge is allowed: GitHub unreachable".to_string()), "{}", text(&o));

    gh.serve(&green());
    for (args, token) in
        [(vec!["-x"], None), (vec!["own/Keel/x"], None), (vec!["a", "b"], None), (vec!["own/Keel"], Some("a b")), (vec!["own/Keel"], Some("a\"b"))]
    {
        let o = gh.run(&args, token);
        assert_eq!(o.status.code(), Some(2), "{args:?} {token:?}: {}", text(&o));
        assert!(gh.log().is_empty(), "{args:?}: no request");
    }
}
