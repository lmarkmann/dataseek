#![expect(missing_docs, reason = "build scripts are not a public API")]

use std::path::Path;
use std::process::Command;

// A crates.io build has no `.git`, and a rerun-if-changed path that does not
// exist reruns the script on every build. A worktree's `.git` is a file, so
// git names the real HEAD and refs.
fn main() {
    let sha = if Path::new(".git").exists() {
        for name in ["HEAD", "refs"] {
            if let Some(path) = git(&["rev-parse", "--git-path", name]) {
                println!("cargo::rerun-if-changed={path}");
            }
        }
        built_from()
    } else {
        println!("cargo::rerun-if-changed=build.rs");
        "unknown".to_owned()
    };
    println!("cargo::rustc-env=DATASEEK_GIT_SHA={sha}");
}

fn built_from() -> String {
    let Some(sha) = git(&["rev-parse", "--short", "HEAD"]) else {
        return "unknown".to_owned();
    };
    match git(&["status", "--porcelain"]) {
        Some(changes) if !changes.is_empty() => format!("{sha}-dirty"),
        _ => sha,
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).ok())
        .flatten()
        .map(|s| s.trim().to_owned())
}
