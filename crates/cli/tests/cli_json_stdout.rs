//! `--json` stdout is a machine contract: skills and CI parse it, so
//! nothing but the JSON document may reach it — including the output of
//! child processes such as `git worktree add` in `heal diff`.

use std::path::Path;
use std::process::{Command, Output};

/// Run `git` with a throwaway identity and no signing, isolated from the
/// developer's global config.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=heal-test",
            "-c",
            "user.email=heal-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn heal(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_heal"))
        .current_dir(dir)
        .args(args)
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("heal runs")
}

#[test]
fn json_commands_write_only_json_to_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn pick(a: i32, b: i32) -> i32 {\n    if a > b { a } else { b }\n}\n",
    )
    .unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "init"]);

    let init = heal(dir, &["init", "--no-skills", "--json"]);
    assert!(
        init.status.success(),
        "heal init: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    for args in [&["status", "--json"][..], &["diff", "HEAD", "--json"][..]] {
        let out = heal(dir, args);
        assert!(
            out.status.success(),
            "heal {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if let Err(e) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            panic!(
                "heal {args:?} stdout is not JSON ({e}):\n{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}
