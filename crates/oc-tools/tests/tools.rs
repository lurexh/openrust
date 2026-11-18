use oc_core::Tool;
use oc_tools::{Bash, Edit, Glob, Read, Write};

fn tmp(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(name);
    p.to_string_lossy().into_owned()
}

#[test]
fn bash_echo() {
    let out = Bash::new(".").execute(r#"{"command":"echo hi"}"#).unwrap();
    assert!(out.contains("hi"));
}

#[test]
fn read_write_edit_roundtrip() {
    let f = tmp("oc_test.txt");
    std::fs::write(&f, "alpha\nbeta\n").unwrap();
    assert!(Read.execute(&format!(r#"{{"path":"{f}"}}"#)).unwrap().contains("2: beta"));
    Write.execute(&format!(r#"{{"path":"{f}","content":"gamma"}}"#)).unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "gamma");
    Edit.execute(&format!(r#"{{"path":"{f}","old":"gamma","new":"delta"}}"#)).unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "delta");
    assert!(Edit.execute(&format!(r#"{{"path":"{f}","old":"nope","new":"x"}}"#)).is_err());
    let _ = std::fs::remove_file(&f);
}

#[test]
fn glob_star_star() {
    let out = Glob.execute(r#"{"pattern":"**/*.rs"}"#).unwrap();
    assert!(out.contains("src/lib.rs"), "got: {out}");
}
