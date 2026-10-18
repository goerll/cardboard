//! Which agent is working on which layers: soft locks that expire, shown in the editor.
//!
//! Agents claim nodes explicitly (`start_working_on_nodes`) or implicitly by editing them. Another
//! agent editing a claimed node, an ancestor that would swallow it, or anything inside it gets an
//! error until the claim is released (`finish_working_on_nodes`) or expires. The person in the
//! editor is never blocked.
use crate::document::{Document, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Claims outlive a pause to think, not an abandoned session.
pub const CLAIM_TTL: Duration = Duration::from_secs(120);

/// An MCP session. `id` survives relay reconnects; `name` is the client's self-reported name.
#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Claim {
    pub actor: Actor,
    pub expires: Instant,
}

/// Claims on one file, by node id.
#[derive(Clone, Debug, Default)]
pub struct Claims(BTreeMap<String, Claim>);

/// How far an edit reaches from its target.
#[derive(Clone, Copy, PartialEq)]
enum Reach {
    /// Changes the node itself (styles, text): blocked by claims on it or an ancestor.
    Node,
    /// Removes or replaces the node with its subtree: also blocked by claims inside it.
    Subtree,
}

impl Claims {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// `{nodeId: agent name}` for the editor.
    pub fn to_json(&self) -> Value {
        self.0
            .iter()
            .map(|(id, c)| (id.clone(), json!(c.actor.name)))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
    /// Drop expired claims and claims on nodes that no longer exist. Returns whether any changed.
    pub fn prune(&mut self, doc: &Document, now: Instant) -> bool {
        let before = self.0.len();
        self.0
            .retain(|id, c| c.expires > now && doc.node(id).is_ok());
        self.0.len() != before
    }
    /// The first claim held by someone else that an edit of `targets` would trample.
    fn conflict(
        &self,
        doc: &Document,
        actor: &Actor,
        targets: &[(String, Reach)],
    ) -> Option<String> {
        let now = Instant::now();
        for (target, reach) in targets {
            let Ok(node) = doc.node(target) else { continue };
            for (claimed, claim) in &self.0 {
                if claim.actor.id == actor.id || claim.expires <= now {
                    continue;
                }
                let covers = doc.node(claimed).is_ok_and(|c| c.contains(target));
                let inside = *reach == Reach::Subtree && node.contains(claimed);
                if covers || inside {
                    let left = claim.expires.saturating_duration_since(now).as_secs();
                    return Some(format!(
                        "{target} is being edited by another agent ({}, claim on {claimed} expires in {left}s). Work elsewhere, ask the person, or wait for it to finish.",
                        claim.actor.name
                    ));
                }
            }
        }
        None
    }
    fn claim(&mut self, actor: &Actor, ids: impl IntoIterator<Item = String>) {
        let expires = Instant::now() + CLAIM_TTL;
        for id in ids {
            self.0.insert(
                id,
                Claim {
                    actor: actor.clone(),
                    expires,
                },
            );
        }
        // Activity anywhere in the file keeps all of this agent's claims alive.
        for c in self.0.values_mut() {
            if c.actor.id == actor.id {
                c.expires = expires;
            }
        }
    }
    /// Release `ids` (and claims inside them), or everything when `ids` is None.
    pub fn release(&mut self, doc: &Document, actor: &Actor, ids: Option<&[String]>) -> bool {
        let before = self.0.len();
        self.0.retain(|claimed, c| {
            c.actor.id != actor.id
                || ids.is_some_and(|ids| {
                    !ids.iter()
                        .any(|id| id == claimed || doc.node(id).is_ok_and(|n| n.contains(claimed)))
                })
        });
        self.0.len() != before
    }
}

fn ids(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
fn id(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}
fn updates(args: &Value) -> impl Iterator<Item = &Value> {
    args.get("updates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// The nodes a tool call edits, and how far each edit reaches.
fn targets(doc: &Document, name: &str, args: &Value) -> Vec<(String, Reach)> {
    use Reach::*;
    let mut out = vec![];
    match name {
        "write_html" | "insert_icon" => {
            let reach = match args.get("mode").and_then(Value::as_str) {
                Some("replace") => Subtree,
                _ => Node,
            };
            out.extend(id(args, "targetNodeId").map(|t| (t, reach)));
            out.extend(id(args, "nodeId").map(|t| (t, Node)));
        }
        "update_styles" | "set_attributes" => {
            for u in updates(args) {
                out.extend(ids(u, "nodeIds").into_iter().map(|i| (i, Node)));
                out.extend(id(u, "nodeId").map(|i| (i, Node)));
                out.extend(
                    crate::tools::query_ids(doc, u)
                        .into_iter()
                        .map(|i| (i, Node)),
                );
            }
        }
        "set_text_content" | "rename_nodes" | "set_node_state" | "set_positions" => {
            out.extend(
                updates(args)
                    .filter_map(|u| id(u, "nodeId"))
                    .map(|i| (i, Node)),
            );
        }
        // Moving the main to another page takes its whole subtree along.
        "create_component" => {
            let reach = if args.get("moveToPage").is_some() {
                Subtree
            } else {
                Node
            };
            out.extend(id(args, "nodeId").map(|i| (i, reach)));
        }
        "insert_instance" => out.extend(id(args, "parentId").map(|p| (p, Node))),
        "detach_instance" | "reset_overrides" => {
            out.extend(ids(args, "nodeIds").into_iter().map(|i| (i, Subtree)));
        }
        "delete_nodes" | "start_working_on_nodes" => {
            out.extend(ids(args, "nodeIds").into_iter().map(|i| (i, Subtree)));
            out.extend(
                crate::tools::query_ids(doc, args)
                    .into_iter()
                    .map(|i| (i, Subtree)),
            );
        }
        "duplicate_nodes" => {
            for spec in args
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                out.extend(id(spec, "parentId").map(|p| (p, Node)));
            }
        }
        "move_nodes" => {
            for m in args
                .get("moves")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                out.extend(id(m, "nodeId").map(|n| (n, Subtree)));
                out.extend(id(m, "parentId").map(|p| (p, Node)));
            }
        }
        "delete_pages" => {
            for page in ids(args, "pageIds") {
                if let Some(p) = doc.pages.iter().find(|p| p.id == page) {
                    out.extend(p.artboards.iter().map(|a| (a.id.clone(), Subtree)));
                }
            }
        }
        "batch" => {
            for op in args
                .get("operations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let tool = op.get("tool").and_then(Value::as_str).unwrap_or_default();
                out.extend(targets(doc, tool, op.get("args").unwrap_or(&Value::Null)));
            }
        }
        _ => {}
    }
    out
}

/// Nodes an agent holds after a successful call: what it edited plus what it created.
fn touched(doc: &Document, name: &str, args: &Value, result: &Value) -> Vec<String> {
    let mut out = vec![];
    match name {
        // Adding to a container holds only the new layers, so others can keep working beside them.
        "write_html" | "create_kit" => out.extend(ids(result, "nodeIds")),
        "create_artboard" | "insert_icon" | "insert_instance" => out.extend(id(result, "nodeId")),
        "duplicate_nodes" => out.extend(
            result
                .get("duplicates")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|d| id(d, "nodeId")),
        ),
        "batch" => {
            let ops = args.get("operations").and_then(Value::as_array);
            let results = result.get("results").and_then(Value::as_array);
            for (op, r) in ops.into_iter().flatten().zip(results.into_iter().flatten()) {
                let tool = op.get("tool").and_then(Value::as_str).unwrap_or_default();
                out.extend(touched(
                    doc,
                    tool,
                    op.get("args").unwrap_or(&Value::Null),
                    r,
                ));
            }
        }
        "delete_nodes" | "delete_pages" => {}
        "create_component" => {
            out.extend(id(args, "nodeId"));
            out.extend(id(result, "instanceId"));
        }
        // A move holds the moved layers, not their new parent.
        "move_nodes" => {
            out.extend(
                args.get("moves")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|m| id(m, "nodeId")),
            );
        }
        _ => out.extend(targets(doc, name, args).into_iter().map(|(id, _)| id)),
    }
    out
}

/// Refuse an agent's edit that would trample another agent's claim. Returns the nodes that the
/// call's `query` items matched before the edit, since an edit can change what a query matches.
pub fn check(
    doc: &Document,
    claims: &Claims,
    actor: &Actor,
    name: &str,
    args: &Value,
) -> Result<Vec<String>> {
    if let Some(message) = claims.conflict(doc, actor, &targets(doc, name, args)) {
        return Err(message);
    }
    Ok(queried(doc, name, args))
}

/// Nodes matched by the `query` items of a style or attribute edit (including inside `batch`).
fn queried(doc: &Document, name: &str, args: &Value) -> Vec<String> {
    match name {
        "update_styles" | "set_attributes" => updates(args)
            .flat_map(|u| crate::tools::query_ids(doc, u))
            .collect(),
        "batch" => args
            .get("operations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|op| {
                let tool = op.get("tool").and_then(Value::as_str).unwrap_or_default();
                queried(doc, tool, op.get("args").unwrap_or(&Value::Null))
            })
            .collect(),
        _ => vec![],
    }
}

/// Record what an agent's successful edit touched.
pub fn record(
    doc: &Document,
    claims: &mut Claims,
    actor: &Actor,
    name: &str,
    args: &Value,
    result: &Value,
    queried: Vec<String>,
) {
    let ids = touched(doc, name, args, result)
        .into_iter()
        .chain(queried)
        .filter(|id| doc.node(id).is_ok());
    claims.claim(actor, ids);
}
