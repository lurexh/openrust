//! Native Rust knowledge-graph memory — full replacement for the node
//! `@modelcontextprotocol/server-memory` package. Persists to one JSON file.

use anyhow::Result;
use oc_core::{Tool, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Entity {
    pub name: String,
    #[serde(rename = "entityType")]
    pub entity_type: String,
    #[serde(default)]
    pub observations: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Relation {
    pub from: String,
    pub to: String,
    #[serde(rename = "relationType")]
    pub relation_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub relations: Vec<Relation>,
}

pub struct Memory {
    path: PathBuf,
    graph: Graph,
}

impl Memory {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let graph = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(Self { path, graph })
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_string(&self.graph)?)?;
        Ok(())
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.graph.entities.iter().position(|e| e.name == name)
    }

    pub fn create_entities(&mut self, entities: Vec<Entity>) -> Vec<String> {
        let mut created = Vec::new();
        for e in entities {
            if self.find(&e.name).is_none() {
                created.push(e.name.clone());
                self.graph.entities.push(e);
            }
        }
        let _ = self.save();
        created
    }

    pub fn create_relations(&mut self, relations: Vec<Relation>) -> Vec<Relation> {
        let mut created = Vec::new();
        for r in relations {
            let exists = self
                .graph
                .relations
                .iter()
                .any(|x| x.from == r.from && x.to == r.to && x.relation_type == r.relation_type);
            if !exists {
                created.push(r.clone());
                self.graph.relations.push(r);
            }
        }
        let _ = self.save();
        created
    }

    /// Returns the observations that were actually added (dedup per entity).
    pub fn add_observations(
        &mut self,
        items: Vec<(String, Vec<String>)>,
    ) -> Result<Vec<(String, Vec<String>)>> {
        let mut added = Vec::new();
        for (name, observations) in items {
            let Some(idx) = self.find(&name) else {
                return Err(anyhow::anyhow!("entity `{name}` not found"));
            };
            let e = &mut self.graph.entities[idx];
            let fresh: Vec<String> = observations
                .into_iter()
                .filter(|o| !e.observations.contains(o))
                .collect();
            if !fresh.is_empty() {
                added.push((name.clone(), fresh.clone()));
                e.observations.extend(fresh);
            }
        }
        let _ = self.save();
        Ok(added)
    }

    pub fn delete_observations(&mut self, items: Vec<(String, Vec<String>)>) {
        for (name, observations) in items {
            if let Some(idx) = self.find(&name) {
                let e = &mut self.graph.entities[idx];
                e.observations.retain(|o| !observations.contains(o));
            }
        }
        let _ = self.save();
    }

    /// Removes entities and any relations touching them.
    pub fn delete_entities(&mut self, names: &[String]) {
        self.graph.entities.retain(|e| !names.contains(&e.name));
        self.graph
            .relations
            .retain(|r| !names.contains(&r.from) && !names.contains(&r.to));
        let _ = self.save();
    }

    pub fn delete_relations(&mut self, relations: Vec<Relation>) {
        self.graph.relations.retain(|r| {
            !relations
                .iter()
                .any(|x| x.from == r.from && x.to == r.to && x.relation_type == r.relation_type)
        });
        let _ = self.save();
    }

    fn relations_touching(&self, names: &[String]) -> Vec<Relation> {
        self.graph
            .relations
            .iter()
            .filter(|r| names.contains(&r.from) || names.contains(&r.to))
            .cloned()
            .collect()
    }

    pub fn search_nodes(&self, query: &str) -> Graph {
        let q = query.to_lowercase();
        let entities: Vec<Entity> = self
            .graph
            .entities
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.entity_type.to_lowercase().contains(&q)
                    || e.observations.iter().any(|o| o.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
        Graph { entities, relations: self.relations_touching(&names) }
    }

    pub fn open_nodes(&self, names: &[String]) -> Graph {
        let entities: Vec<Entity> = self
            .graph
            .entities
            .iter()
            .filter(|e| names.contains(&e.name))
            .cloned()
            .collect();
        let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
        Graph { entities, relations: self.relations_touching(&names) }
    }
}

// ---------- tool plumbing ----------

fn args(json: &str) -> Result<Value> {
    Ok(serde_json::from_str(json)?)
}

type Shared = Arc<Mutex<Memory>>;

fn spec(name: &str, description: &str, schema: Value) -> ToolSpec {
    serde_json::from_value(serde_json::json!({
        "type": "function",
        "function": {"name": name, "description": description, "parameters": schema}
    }))
    .unwrap()
}

macro_rules! tool_struct {
    ($t:ident, $name:literal, $desc:literal, $schema:literal, $exec:expr) => {
        pub struct $t(pub Shared);
        impl Tool for $t {
            fn spec(&self) -> ToolSpec {
                spec($name, $desc, serde_json::from_str($schema).unwrap())
            }
            fn execute(&self, a: &str) -> Result<String> {
                let v = args(a)?;
                let mut m = self.0.lock().unwrap();
                let out: Value = ($exec)(v, &mut m)?;
                Ok(serde_json::to_string(&out)?)
            }
        }
    };
}

tool_struct!(
    CreateEntities,
    "memory_create_entities",
    "Create new entities in the knowledge graph (existing names are skipped)",
    r#"{"type":"object","properties":{"entities":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"entityType":{"type":"string"},"observations":{"type":"array","items":{"type":"string"}}},"required":["name","entityType","observations"]}}},"required":["entities"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let items: Vec<Entity> = serde_json::from_value(v["entities"].clone())?;
    Ok(serde_json::json!(m.create_entities(items)))
}
);

tool_struct!(
    CreateRelations,
    "memory_create_relations",
    "Create relations between entities (active voice, e.g. 'uses', 'depends on')",
    r#"{"type":"object","properties":{"relations":{"type":"array","items":{"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"},"relationType":{"type":"string"}},"required":["from","to","relationType"]}}},"required":["relations"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let items: Vec<Relation> = serde_json::from_value(v["relations"].clone())?;
    Ok(serde_json::json!(m.create_relations(items)))
}
);

tool_struct!(
    AddObservations,
    "memory_add_observations",
    "Add new observations to existing entities (duplicates skipped)",
    r#"{"type":"object","properties":{"observations":{"type":"array","items":{"type":"object","properties":{"entityName":{"type":"string"},"contents":{"type":"array","items":{"type":"string"}}},"required":["entityName","contents"]}}},"required":["observations"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let raw = v["observations"].as_array().cloned().unwrap_or_default();
    let items: Vec<(String, Vec<String>)> = raw
        .into_iter()
        .filter_map(|o| {
            Some((
                o["entityName"].as_str()?.to_string(),
                o["contents"]
                    .as_array()?
                    .iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect(),
            ))
        })
        .collect();
    let added = m.add_observations(items)?;
    Ok(serde_json::json!(added
        .into_iter()
        .map(|(n, cs)| serde_json::json!({"entityName": n, "contents": cs}))
        .collect::<Vec<_>>()))
}
);

tool_struct!(
    DeleteEntities,
    "memory_delete_entities",
    "Delete entities and their relations from the knowledge graph",
    r#"{"type":"object","properties":{"entityNames":{"type":"array","items":{"type":"string"}}},"required":["entityNames"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let names: Vec<String> = serde_json::from_value(v["entityNames"].clone())?;
    m.delete_entities(&names);
    Ok(serde_json::json!(names))
}
);

tool_struct!(
    DeleteObservations,
    "memory_delete_observations",
    "Delete specific observations from entities (exact text match)",
    r#"{"type":"object","properties":{"deletions":{"type":"array","items":{"type":"object","properties":{"entityName":{"type":"string"},"observations":{"type":"array","items":{"type":"string"}}},"required":["entityName","observations"]}}},"required":["deletions"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let raw = v["deletions"].as_array().cloned().unwrap_or_default();
    let items: Vec<(String, Vec<String>)> = raw
        .into_iter()
        .filter_map(|o| {
            Some((
                o["entityName"].as_str()?.to_string(),
                o["observations"]
                    .as_array()?
                    .iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect(),
            ))
        })
        .collect();
    m.delete_observations(items);
    Ok(serde_json::json!("deleted"))
}
);

tool_struct!(
    DeleteRelations,
    "memory_delete_relations",
    "Delete specific relations (exact from/to/relationType match)",
    r#"{"type":"object","properties":{"relations":{"type":"array","items":{"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"},"relationType":{"type":"string"}},"required":["from","to","relationType"]}}},"required":["relations"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let items: Vec<Relation> = serde_json::from_value(v["relations"].clone())?;
    m.delete_relations(items);
    Ok(serde_json::json!("deleted"))
}
);

tool_struct!(
    ReadGraph,
    "memory_read_graph",
    "Read the entire knowledge graph",
    r#"{"type":"object","properties":{}}"#,
    |_v: Value, m: &mut Memory| -> Result<Value> {
    Ok(serde_json::json!(m.graph()))
}
);

tool_struct!(
    SearchNodes,
    "memory_search_nodes",
    "Search the knowledge graph by keyword (matches names, types, observations)",
    r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    Ok(serde_json::json!(m.search_nodes(v["query"].as_str().unwrap_or(""))))
}
);

tool_struct!(
    OpenNodes,
    "memory_open_nodes",
    "Open specific entities by exact name, with their relations",
    r#"{"type":"object","properties":{"names":{"type":"array","items":{"type":"string"}}},"required":["names"]}"#,
    |v: Value, m: &mut Memory| -> Result<Value> {
    let names: Vec<String> = serde_json::from_value(v["names"].clone())?;
    Ok(serde_json::json!(m.open_nodes(&names)))
}
);

/// All 9 memory tools sharing one store.
pub fn tools(path: impl Into<PathBuf>) -> Result<Vec<Box<dyn Tool>>> {
    let mem: Shared = Arc::new(Mutex::new(Memory::open(path)?));
    Ok(vec![
        Box::new(CreateEntities(Arc::clone(&mem))),
        Box::new(CreateRelations(Arc::clone(&mem))),
        Box::new(AddObservations(Arc::clone(&mem))),
        Box::new(DeleteEntities(Arc::clone(&mem))),
        Box::new(DeleteObservations(Arc::clone(&mem))),
        Box::new(DeleteRelations(Arc::clone(&mem))),
        Box::new(ReadGraph(Arc::clone(&mem))),
        Box::new(SearchNodes(Arc::clone(&mem))),
        Box::new(OpenNodes(mem)),
    ])
}
