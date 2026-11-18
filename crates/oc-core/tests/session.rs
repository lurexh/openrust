use oc_core::session;
use oc_core::{Message, Role};
use std::path::Path;

#[test]
fn session_roundtrip_per_cwd() {
    let base = std::env::temp_dir().join(format!("oc_sess_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    let a = Path::new("/proj/alpha");
    let b = Path::new("/proj/beta");

    let history = vec![Message::system("sys"), Message::user("hello"), Message::assistant("world", vec![])];
    session::save_history_in(&base, a, &history);
    session::save_history_in(&base, b, &vec![Message::system("s")]);

    let loaded = session::load_history_in(&base, a);
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[2].role, Role::Assistant);
    assert_eq!(loaded[2].content.as_deref(), Some("world"));

    // separate sessions stay separate
    assert_eq!(session::load_history_in(&base, b).len(), 1);

    session::clear_history_in(&base, a);
    assert!(session::load_history_in(&base, a).is_empty());
    assert_eq!(session::load_history_in(&base, b).len(), 1);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn load_missing_session_is_empty() {
    let base = std::env::temp_dir().join("oc_sess_missing");
    assert!(session::load_history_in(&base, Path::new("/nowhere")).is_empty());
}
