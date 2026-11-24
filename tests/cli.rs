//! End-to-end checks for the non-GUI command line: help, version, keys, and
//! usage errors. The viewer itself needs a window, so it is not exercised here.

use std::process::{Command, Output};

fn jr(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jr"))
        .args(args)
        .output()
        .expect("run the jr binary")
}

#[test]
fn help_prints_usage_and_exits_zero() {
    let out = jr(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Usage: jr"), "unexpected help: {text}");
}

#[test]
fn version_lists_mermaid_and_chromium() {
    let out = jr(&["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!("jiaoren {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("mermaid "));
    assert!(text.contains("chromium "));
}

#[test]
fn keys_prints_the_shortcut_list() {
    let out = jr(&["--keys"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("gg / G"));
}

#[test]
fn missing_file_exits_with_status_two() {
    let out = jr(&["/nonexistent/definitely-not-a-document.md"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty());
}

#[test]
fn unknown_flag_exits_with_status_two() {
    let out = jr(&["--no-such-flag"]);
    assert_eq!(out.status.code(), Some(2));
}
