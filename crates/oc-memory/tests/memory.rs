use oc_memory::{Graph, Memory, tools};
use serde_json::json;
use std::path::Path;

#[test]
fn entities_relations_crud_and_dedup() {
    let dir = std::env::temp_dir().join(format!("oc_mem_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory.json");

    let mut m = Memory::open(&path).unwrap();
    let created = m.create_entities(vec![
        oc_memory::Entity {
            name: "openrust".into(),
            entity_type: "project".into(),
            observations: vec!["Rust rewrite".into()],
        },
        oc_memory::Entity {
            name: "openrust".into(), // duplicate name skipped
            entity_type: "thing".into(),
            observations: vec!["again".into()],
        },
    ]);
    assert_eq!(created.len(), 1);
    assert_eq!(m.graph().entities.len(), 1);

    let rels = m.create_relations(vec![oc_memory::Relation {
        from: "openrust".into(),
        to: "rust".into(),
        relation_type: "written in".into(),
    }]);
    assert_eq!(rels.len(), 1);
    assert_eq!(m.create_relations(rels).len(), 0, "duplicate relation skipped");

    // add_observations dedups
    let added = m
        .add_observations(vec![(
            "openrust".into(),
            vec!["Rust rewrite".into(), "MIT licensed".into()],
        )])
        .unwrap();
    assert_eq!(added[0].1, vec!["MIT licensed"]);
    assert_eq!(m.graph().entities[0].observations.len(), 2);

    // unknown entity errors
    assert!(m.add_observations(vec![("nope".into(), vec!["x".into()])]).is_err());

    // delete observations
    m.delete_observations(vec![("openrust".into(), vec!["Rust rewrite".into()])]);
    assert_eq!(m.graph().entities[0].observations, vec!["MIT licensed"]);

    // delete_relations
    m.delete_relations(vec![oc_memory::Relation {
        from: "openrust".into(),
        to: "rust".into(),
        relation_type: "written in".into(),
    }]);
    assert!(m.graph().relations.is_empty());

    // delete_entities + dangling relation cleanup
    m.create_relations(vec![oc_memory::Relation {
        from: "openrust".into(),
        to: "rust".into(),
        relation_type: "written in".into(),
    }]);
    m.delete_entities(&["openrust".into()]);
    assert!(m.graph().entities.is_empty());
    assert!(m.graph().relations.is_empty(), "dangling relations removed");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn search_and_open_nodes() {
    let dir = std::env::temp_dir().join(format!("oc_mem_s_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut m = Memory::open(dir.join("memory.json")).unwrap();
    m.create_entities(vec![
        oc_memory::Entity { name: "alpha".into(), entity_type: "tool".into(), observations: vec!["does streaming".into()] },
        oc_memory::Entity { name: "beta".into(), entity_type: "server".into(), observations: vec!["rust native".into()] },
    ]);
    m.create_relations(vec![oc_memory::Relation {
        from: "alpha".into(),
        to: "beta".into(),
        relation_type: "connects to".into(),
    }]);

    // search matches by observation content (case-insensitive)
    let g: Graph = m.search_nodes("STREAMING");
    assert_eq!(g.entities.len(), 1);
    assert_eq!(g.entities[0].name, "alpha");
    assert_eq!(g.relations.len(), 1, "touching relations included");

    // search by type
    assert_eq!(m.search_nodes("server").entities[0].name, "beta");

    // open_nodes exact
    let g = m.open_nodes(&["beta".into(), "missing".into()]);
    assert_eq!(g.entities.len(), 1);
    assert_eq!(g.relations.len(), 1, "alpha->beta touches beta");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn persistence_roundtrip() {
    let dir = std::env::temp_dir().join(format!("oc_mem_p_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memory.json");

    {
        let mut m = Memory::open(&path).unwrap();
        m.create_entities(vec![oc_memory::Entity {
            name: "persist".into(),
            entity_type: "test".into(),
            observations: vec!["survives restart".into()],
        }]);
    }
    // fresh instance loads from disk
    let m = Memory::open(&path).unwrap();
    assert_eq!(m.graph().entities[0].name, "persist");
    assert_eq!(m.graph().entities[0].observations, vec!["survives restart"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn builtin_tool_wrappers_roundtrip() {
    let dir = std::env::temp_dir().join(format!("oc_mem_t_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let all = tools(dir.join("memory.json")).unwrap();
    assert_eq!(all.len(), 9);

    let mut by_name = |n: &str| {
        all.iter()
            .find(|t| t.spec().function.name == n)
            .unwrap_or_else(|| panic!("missing {n}"))
    };

    // names match server-memory's toolset (with memory_ prefix)
    for n in [
        "memory_create_entities",
        "memory_create_relations",
        "memory_add_observations",
        "memory_delete_entities",
        "memory_delete_observations",
        "memory_delete_relations",
        "memory_read_graph",
        "memory_search_nodes",
        "memory_open_nodes",
    ] {
        let _ = by_name(n);
    }

    by_name("memory_create_entities")
        .execute(&json!({"entities":[{"name":"x","entityType":"t","observations":["o1"]}]}).to_string())
        .unwrap();
    by_name("memory_add_observations")
        .execute(&json!({"observations":[{"entityName":"x","contents":["o2"]}]}).to_string())
        .unwrap();
    by_name("memory_create_relations")
        .execute(&json!({"relations":[{"from":"x","to":"y","relationType":"likes"}]}).to_string())
        .unwrap();

    let graph = by_name("memory_read_graph").execute("{}").unwrap();
    assert!(graph.contains("\"x\""));
    let found = by_name("memory_search_nodes").execute(&json!({"query":"o2"}).to_string()).unwrap();
    assert!(found.contains("\"x\""));
    let opened = by_name("memory_open_nodes").execute(&json!({"names":["x"]}).to_string()).unwrap();
    assert!(opened.contains("o1"));
    assert!(opened.contains("\"likes\""), "touching relation included");
    assert!(!opened.contains("\"y\":"), "neighbor entity y not returned");

    by_name("memory_delete_observations")
        .execute(&json!({"deletions":[{"entityName":"x","observations":["o1"]}]}).to_string())
        .unwrap();
    by_name("memory_delete_relations")
        .execute(&json!({"relations":[{"from":"x","to":"y","relationType":"likes"}]}).to_string())
        .unwrap();
    by_name("memory_delete_entities")
        .execute(&json!({"entityNames":["x"]}).to_string())
        .unwrap();

    // graph empty again, persisted to disk
    let path = dir.join("memory.json");
    assert!(Path::new(&path).exists());
    let m = Memory::open(&path).unwrap();
    assert!(m.graph().entities.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}
