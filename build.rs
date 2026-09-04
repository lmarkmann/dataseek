#![expect(missing_docs, reason = "build scripts are not a public API")]

use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=.git/HEAD");
    println!("cargo::rerun-if-changed=.git/refs");
    println!("cargo::rustc-env=DATASEEK_GIT_SHA={}", built_from());
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
