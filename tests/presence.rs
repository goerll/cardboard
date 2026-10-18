use cardboard::{
    document::Store,
    presence::{Actor, CLAIM_TTL},
    tools,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn agent(id: &str) -> Option<Actor> {
    Some(Actor {
        id: id.into(),
        name: format!("agent {id}"),
    })
}
/// Run a tool as `actor` (None: the person in the editor).
fn run(s: &mut Store, actor: &Option<Actor>, name: &str, args: Value) -> Result<Value, String> {
    s.actor = actor.clone();
    let result = tools::call(s, name, &args);
    s.actor = None;
    result
}
fn ok(s: &mut Store, actor: &Option<Actor>, name: &str, args: Value) -> Value {
    run(s, actor, name, args).unwrap()
}
fn fid(s: &Store) -> String {
    s.active_file.clone().unwrap()
}
fn artboard(s: &mut Store, actor: &Option<Actor>) -> String {
    ok(
        s,
        actor,
        "create_artboard",
        json!({"name":"Frame","styles":{"width":"100px","height":"100px"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .into()
}
fn color(id: &str) -> Value {
    json!({"updates":[{"nodeIds":[id],"styles":{"color":"red"}}]})
}

#[test]
fn another_agent_cannot_edit_claimed_layers_until_they_are_released() {
    let mut s = Store::memory();
    let (a, b) = (agent("a"), agent("b"));
    let frame = artboard(&mut s, &a);
    let child = ok(
        &mut s,
        &b,
        "create_artboard",
        json!({"name":"Other","styles":{"width":"10px","height":"10px"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    // Anything inside A's artboard is A's for now, including adding to it.
    let error = run(&mut s, &b, "update_styles", color(&frame)).unwrap_err();
    assert!(error.contains("agent a"), "{error}");
    let insert = json!({"targetNodeId":frame,"mode":"insert-children","html":"<p>x</p>"});
    assert!(run(&mut s, &b, "write_html", insert.clone()).is_err());
    // Batches are checked operation by operation.
    let batch = json!({"operations":[{"tool":"update_styles","args":color(&frame)}]});
    assert!(run(&mut s, &b, "batch", batch).is_err());
    // A can keep going, the person is never blocked, and B can work elsewhere.
    let inner = ok(&mut s, &a, "write_html", insert)["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    ok(&mut s, &None, "update_styles", color(&inner));
    ok(&mut s, &b, "update_styles", color(&child));
    let agents = s.files[&fid(&s)].claims.to_json();
    assert_eq!(agents[&frame], "agent a");
    assert_eq!(agents[&child], "agent b");

    ok(
        &mut s,
        &a,
        "finish_working_on_nodes",
        json!({"nodeIds":[frame]}),
    );
    assert!(s.files[&fid(&s)].claims.to_json().get(&frame).is_none());
    assert!(s.files[&fid(&s)].claims.to_json().get(&inner).is_none());
    ok(&mut s, &b, "update_styles", color(&inner));
}

#[test]
fn removing_a_layer_that_contains_a_claim_is_blocked() {
    let mut s = Store::memory();
    let (a, b) = (agent("a"), agent("b"));
    let frame = artboard(&mut s, &None);
    let inner = ok(
        &mut s,
        &a,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":"<div><p>a</p></div>"}),
    )["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    // The container itself is unclaimed: B may style it, but not delete or replace it.
    ok(&mut s, &b, "update_styles", color(&frame));
    assert!(run(&mut s, &b, "delete_nodes", json!({"nodeIds":[frame]})).is_err());
    let replace = json!({"targetNodeId":frame,"mode":"replace","html":"<div></div>"});
    assert!(run(&mut s, &b, "write_html", replace).is_err());
    assert!(
        run(
            &mut s,
            &b,
            "start_working_on_nodes",
            json!({"nodeIds":[frame]})
        )
        .is_err()
    );
    let page = s.file(&fid(&s)).unwrap().active_page.clone();
    ok(&mut s, &None, "create_page", json!({}));
    assert!(run(&mut s, &b, "delete_pages", json!({"pageIds":[page]})).is_err());
    ok(&mut s, &a, "finish_working_on_nodes", json!({}));
    ok(
        &mut s,
        &b,
        "start_working_on_nodes",
        json!({"nodeIds":[frame]}),
    );
    assert!(run(&mut s, &a, "update_styles", color(&inner)).is_err());
}

#[test]
fn claims_expire() {
    let mut s = Store::memory();
    let frame = artboard(&mut s, &agent("a"));
    let id = fid(&s);
    let file = s.files.get_mut(&id).unwrap();
    assert!(!file.claims.prune(&file.doc, Instant::now()));
    let later = Instant::now() + CLAIM_TTL + Duration::from_secs(1);
    assert!(file.claims.prune(&file.doc, later));
    assert!(file.claims.is_empty());
    ok(&mut s, &agent("b"), "update_styles", color(&frame));
}

#[test]
fn an_agent_only_undoes_its_own_edits() {
    let mut s = Store::memory();
    let (a, b) = (agent("a"), agent("b"));
    let first = artboard(&mut s, &a);
    let second = artboard(&mut s, &b);
    let error = run(&mut s, &a, "undo", json!({})).unwrap_err();
    assert!(error.contains("agent b"), "{error}");
    ok(&mut s, &b, "undo", json!({}));
    assert!(s.file(&fid(&s)).unwrap().node(&second).is_err());
    // B's undone step is B's to redo; A's edit is now on top again.
    assert!(run(&mut s, &a, "redo", json!({})).is_err());
    ok(&mut s, &a, "undo", json!({}));
    assert!(s.file(&fid(&s)).unwrap().node(&first).is_err());
    // The person's edits are protected from agents, but the person can undo anything.
    artboard(&mut s, &a);
    ok(
        &mut s,
        &None,
        "update_pages",
        json!({"updates":[{"pageId":"page-1","name":"P"}]}),
    );
    assert!(
        run(&mut s, &a, "undo", json!({}))
            .unwrap_err()
            .contains("person")
    );
    ok(&mut s, &None, "undo", json!({}));
    ok(&mut s, &None, "undo", json!({}));
}

#[test]
fn query_targeted_edits_respect_and_take_claims() {
    let mut s = Store::memory();
    let (a, b) = (agent("a"), agent("b"));
    let frame = artboard(&mut s, &a);
    let insert = json!({"targetNodeId":frame,"mode":"insert-children","html":"<p style=\"color:blue\">x</p>"});
    let p = ok(&mut s, &a, "write_html", insert)["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    // A query reaching A's layer is refused, alone or inside a batch.
    let by_color = json!({"styles":{"color":"blue"}});
    let restyle = json!({"updates":[{"query":by_color,"styles":{"color":"red"}}]});
    let error = run(&mut s, &b, "update_styles", restyle.clone()).unwrap_err();
    assert!(error.contains("agent a"), "{error}");
    let batch = json!({"operations":[{"tool":"set_attributes","args":{"updates":[{"query":by_color,"attributes":{"title":"t"}}]}}]});
    assert!(run(&mut s, &b, "batch", batch).is_err());
    assert!(run(&mut s, &b, "delete_nodes", json!({"query":by_color})).is_err());
    // Once released, B's query edit claims what it matched, even though it no longer matches.
    ok(&mut s, &a, "finish_working_on_nodes", json!({}));
    ok(&mut s, &b, "update_styles", restyle);
    let error = run(&mut s, &a, "update_styles", color(&p)).unwrap_err();
    assert!(error.contains("agent b"), "{error}");
}

#[test]
fn component_tools_respect_and_take_claims() {
    let mut s = Store::memory();
    let (a, b) = (agent("a"), agent("b"));
    let frame = artboard(&mut s, &a);
    let insert = json!({"targetNodeId":frame,"mode":"insert-children","html":"<button><span>Go</span></button>"});
    let main = ok(&mut s, &a, "write_html", insert)["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    // B cannot turn A's layer into a component, nor place instances inside A's frame.
    assert!(run(&mut s, &b, "create_component", json!({"nodeId":main})).is_err());
    ok(&mut s, &a, "create_component", json!({"nodeId":main}));
    let into_frame = json!({"componentId":main,"parentId":frame});
    assert!(run(&mut s, &b, "insert_instance", into_frame.clone()).is_err());
    // A's new instance is A's: B can neither override its layers nor detach it.
    let inst = ok(&mut s, &a, "insert_instance", into_frame)["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let label = s.file(&fid(&s)).unwrap().node(&inst).unwrap().children[0]
        .id
        .clone();
    let error = run(&mut s, &b, "update_styles", color(&label)).unwrap_err();
    assert!(error.contains("agent a"), "{error}");
    assert!(run(&mut s, &b, "detach_instance", json!({"nodeIds":[inst]})).is_err());
    assert!(run(&mut s, &b, "reset_overrides", json!({"nodeIds":[inst]})).is_err());
    ok(&mut s, &a, "finish_working_on_nodes", json!({}));
    ok(&mut s, &b, "update_styles", color(&label));
}
