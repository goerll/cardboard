use cardboard::{
    document::{Document, Node, Store},
    html, tools,
};
use serde_json::{Value, json};

fn edit(s: &mut Store, name: &str, args: Value) -> Value {
    tools::call(s, name, &args).unwrap()
}
fn doc(s: &Store) -> &Document {
    s.file(s.active_file.as_ref().unwrap()).unwrap()
}
fn node<'a>(s: &'a Store, id: &str) -> &'a Node {
    doc(s).node(id).unwrap()
}
fn artboard(s: &mut Store, name: &str) -> String {
    edit(
        s,
        "create_artboard",
        json!({"name":name,"styles":{"width":"800px","height":"600px"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .into()
}
fn insert(s: &mut Store, parent: &str, html: &str) -> Vec<String> {
    serde_json::from_value(
        edit(
            s,
            "write_html",
            json!({"html":html,"targetNodeId":parent,"mode":"insert-children"}),
        )["nodeIds"]
            .clone(),
    )
    .unwrap()
}
fn instance(s: &mut Store, component: &str, parent: &str) -> String {
    edit(
        s,
        "insert_instance",
        json!({"componentId":component,"parentId":parent}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .into()
}

/// A frame with a "Button" component (a button holding a label span) and its label id.
fn button(s: &mut Store) -> (String, String, String) {
    let frame = artboard(s, "Screen");
    let main = insert(
        s,
        &frame,
        "<button layer-name=\"Button\" style=\"padding:8px;background:#222\"><span style=\"color:white\">Save</span></button>",
    )[0]
    .clone();
    let label = node(s, &main).children[0].id.clone();
    edit(s, "create_component", json!({"nodeId":main}));
    (frame, main, label)
}

#[test]
fn instances_render_the_main_and_follow_its_edits() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    let b = instance(&mut s, &main, &frame);
    let inner = format!("{a}:{label}");
    assert_eq!(node(&s, &a).name, "Button");
    assert_eq!(node(&s, &inner).text, "Save");
    assert_eq!(
        html::document_html(doc(&s), false)
            .matches(">Save</span>")
            .count(),
        3
    );

    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[main],"styles":{"background":"#0a6"}}]}),
    );
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":label,"textContent":"Submit"}]}),
    );
    for id in [&a, &b] {
        assert_eq!(node(&s, id).styles["background"], "#0a6");
        assert_eq!(node(&s, &format!("{id}:{label}")).text, "Submit");
    }
    let info = edit(&mut s, "get_basic_info", json!({}));
    assert_eq!(info["components"][0]["componentId"], json!(main));
    assert_eq!(info["components"][0]["instances"], 2);
    let tree = edit(&mut s, "get_tree_summary", json!({"nodeId":frame}))["tree"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(tree.contains(&format!("[{main}] (component)")));
    assert!(tree.contains(&format!("[{a}] (instance of {main})")));
    assert!(
        edit(&mut s, "get_jsx", json!({"nodeId":a}))["jsx"]
            .as_str()
            .unwrap()
            .contains("{\"Submit\"}")
    );
}

#[test]
fn overrides_survive_main_edits() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    let inner = format!("{a}:{label}");
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":inner,"textContent":"Cancel"}]}),
    );
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[inner],"styles":{"color":"red"}},{"nodeIds":[a],"styles":{"padding":""}}]}),
    );
    edit(
        &mut s,
        "set_attributes",
        json!({"updates":[{"nodeId":a,"attributes":{"title":"Close"}}]}),
    );
    let overrides = &node(&s, &a).instance.as_ref().unwrap().overrides;
    assert_eq!(overrides[&label].text.as_deref(), Some("Cancel"));
    assert_eq!(overrides[&main].styles["padding"], "");

    // Main edits to overridden properties don't win; other properties still flow through.
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":label,"textContent":"Submit"}]}),
    );
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[label],"styles":{"color":"blue","fontWeight":"600"}},{"nodeIds":[main],"styles":{"padding":"12px","borderRadius":"6px"}}]}),
    );
    let n = node(&s, &inner);
    assert_eq!(n.text, "Cancel");
    assert_eq!(n.styles["color"], "red");
    assert_eq!(n.styles["font-weight"], "600");
    let root = node(&s, &a);
    assert!(!root.styles.contains_key("padding"));
    assert_eq!(root.styles["border-radius"], "6px");
    assert_eq!(root.attributes["title"], "Close");

    // A layer added to the main shows up in instances, and overrides stay attached.
    insert(&mut s, &main, "<span>!</span>");
    assert_eq!(node(&s, &a).children.len(), 2);
    assert_eq!(node(&s, &inner).text, "Cancel");

    edit(&mut s, "reset_overrides", json!({"nodeIds":[a]}));
    assert_eq!(node(&s, &inner).text, "Submit");
    assert_eq!(node(&s, &a).styles["padding"], "12px");
}

#[test]
fn instance_structure_is_locked_but_layers_can_be_hidden() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    let inner = format!("{a}:{label}");
    let before = doc(&s).clone();
    for (tool, args) in [
        ("delete_nodes", json!({"nodeIds":[inner]})),
        (
            "write_html",
            json!({"html":"<i>x</i>","targetNodeId":a,"mode":"insert-children"}),
        ),
        (
            "move_nodes",
            json!({"moves":[{"nodeId":inner,"parentId":frame}]}),
        ),
        (
            "insert_instance",
            json!({"componentId":main,"parentId":main}),
        ),
        ("create_component", json!({"nodeId":inner})),
    ] {
        assert!(tools::call(&mut s, tool, &args).is_err(), "{tool}");
        assert_eq!(*doc(&s), before, "{tool}");
    }
    edit(
        &mut s,
        "set_node_state",
        json!({"updates":[{"nodeId":inner,"visible":false}]}),
    );
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[main],"styles":{"gap":"4px"}}]}),
    );
    assert!(!node(&s, &inner).visible);
}

#[test]
fn detaching_or_deleting_the_main_keeps_the_look() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    let b = instance(&mut s, &main, &frame);
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":format!("{a}:{label}"),"textContent":"Cancel"}]}),
    );
    edit(&mut s, "detach_instance", json!({"nodeIds":[a]}));
    let detached = node(&s, &a).clone();
    assert!(detached.instance.is_none());
    assert_eq!(detached.children[0].text, "Cancel");
    assert!(!detached.children[0].id.contains(':'));
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[main],"styles":{"background":"red"}}]}),
    );
    assert_eq!(node(&s, &a).styles["background"], "#222");
    assert_eq!(node(&s, &b).styles["background"], "red");

    edit(&mut s, "delete_nodes", json!({"nodeIds":[main]}));
    let orphan = node(&s, &b);
    assert!(orphan.instance.is_none());
    assert_eq!(orphan.styles["background"], "red");
    assert_eq!(orphan.children[0].text, "Save");
    edit(&mut s, "undo", json!({}));
    assert!(node(&s, &b).instance.is_some());
    assert!(node(&s, &main).component);
}

#[test]
fn duplicates_of_instances_stay_linked_and_copies_of_mains_are_plain() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":format!("{a}:{label}"),"textContent":"Cancel"}]}),
    );
    let dups = edit(
        &mut s,
        "duplicate_nodes",
        json!({"nodes":[{"id":a},{"id":main}]}),
    )["duplicates"]
        .clone();
    let (copy, main_copy) = (
        dups[0]["nodeId"].as_str().unwrap(),
        dups[1]["nodeId"].as_str().unwrap(),
    );
    assert_eq!(node(&s, &format!("{copy}:{label}")).text, "Cancel");
    assert!(!node(&s, main_copy).component);
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[label],"styles":{"color":"gold"}}]}),
    );
    assert_eq!(node(&s, &format!("{copy}:{label}")).styles["color"], "gold");
    assert_eq!(node(&s, main_copy).children[0].styles["color"], "white");
}

#[test]
fn nested_instances_update_through_their_containing_component() {
    let mut s = Store::memory();
    let (frame, button_main, label) = button(&mut s);
    let card = insert(
        &mut s,
        &frame,
        "<div layer-name=\"Card\"><p>Title</p></div>",
    )[0]
    .clone();
    let nested = instance(&mut s, &button_main, &card);
    edit(&mut s, "create_component", json!({"nodeId":card}));
    let outer = instance(&mut s, &card, &frame);
    let deep = format!("{outer}:{nested}:{label}");
    assert_eq!(node(&s, &deep).text, "Save");

    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":label,"textContent":"Buy"}]}),
    );
    assert_eq!(node(&s, &deep).text, "Buy");
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":deep,"textContent":"Buy now"}]}),
    );
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[label],"styles":{"color":"gold"}}]}),
    );
    assert_eq!(node(&s, &deep).text, "Buy now");
    assert_eq!(node(&s, &deep).styles["color"], "gold");

    // A component that would contain itself is rejected.
    assert!(
        tools::call(
            &mut s,
            "insert_instance",
            &json!({"componentId":card,"parentId":card})
        )
        .is_err()
    );

    // Detaching the outer instance keeps the nested button linked, with its text change.
    edit(&mut s, "detach_instance", json!({"nodeIds":[outer]}));
    let inner_button = node(&s, &outer)
        .children
        .iter()
        .find(|c| c.instance.is_some())
        .unwrap()
        .id
        .clone();
    assert!(!inner_button.contains(':'));
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[label],"styles":{"color":"pink"}}]}),
    );
    let text = node(&s, &format!("{inner_button}:{label}"));
    assert_eq!(text.text, "Buy now");
    assert_eq!(text.styles["color"], "pink");
}

#[test]
fn artboards_move_and_copy_across_pages() {
    let mut s = Store::memory();
    let (frame, main, _) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    let page = edit(&mut s, "create_page", json!({"name":"Two"}))["pageId"]
        .as_str()
        .unwrap()
        .to_owned();
    let copy = edit(
        &mut s,
        "duplicate_nodes",
        json!({"nodes":[{"id":frame,"pageId":page}]}),
    )["duplicates"][0]["nodeId"]
        .as_str()
        .unwrap()
        .to_owned();
    let d = doc(&s);
    let two = d.pages.iter().find(|p| p.id == page).unwrap();
    assert_eq!(two.artboards[0].id, copy);
    let copied_instance = two.artboards[0]
        .children
        .iter()
        .find(|c| c.instance.is_some())
        .unwrap()
        .id
        .clone();
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[main],"styles":{"background":"teal"}}]}),
    );
    assert_eq!(node(&s, &copied_instance).styles["background"], "teal");

    // Moving a whole artboard to another page keeps ids; the instance inside keeps following.
    let other = artboard(&mut s, "Other");
    edit(
        &mut s,
        "move_nodes",
        json!({"moves":[{"nodeId":other,"parentId":"root","pageId":page}]}),
    );
    let two = doc(&s).pages.iter().find(|p| p.id == page).unwrap();
    assert_eq!(two.artboards.len(), 2);
    assert_eq!(two.artboards[1].position.unwrap()[0], 880.0);
    edit(
        &mut s,
        "move_nodes",
        json!({"moves":[{"nodeId":a,"parentId":other}]}),
    );
    assert_eq!(node(&s, &a).styles["background"], "teal");
    edit(&mut s, "undo", json!({}));
    edit(&mut s, "undo", json!({}));
    assert_eq!(doc(&s).pages[0].artboards.len(), 2);
}

#[test]
fn components_are_one_undo_step_in_a_batch_and_survive_a_save() {
    let dir =
        std::env::temp_dir().join(format!("cardboard-test-components-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("design.cardboard");
    let mut s = Store::open_document(&path).unwrap();
    let frame = artboard(&mut s, "Screen");
    let chip = insert(&mut s, &frame, "<span>Chip</span>")[0].clone();
    let revision = doc(&s).revision;
    let result = edit(
        &mut s,
        "batch",
        json!({"operations":[
            {"tool":"create_component","args":{"nodeId":chip,"name":"Chip"}},
            {"tool":"insert_instance","args":{"componentId":chip,"parentId":frame}},
        ]}),
    );
    let a = result["results"][1]["nodeId"].as_str().unwrap().to_owned();
    assert_eq!(doc(&s).revision, revision + 1);
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":a,"textContent":"Other"}]}),
    );
    let reopened = Store::open_document(&path).unwrap();
    let saved = reopened
        .file(reopened.active_file.as_ref().unwrap())
        .unwrap();
    assert_eq!(saved, doc(&s));
    assert_eq!(saved.node(&a).unwrap().text, "Other");
    edit(&mut s, "undo", json!({}));
    edit(&mut s, "undo", json!({}));
    assert!(!node(&s, &chip).component);
    assert!(doc(&s).node(&a).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bulk_query_edits_change_mains_without_pinning_instances() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    let b = instance(&mut s, &main, &frame);
    // B pins its own label color first.
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[format!("{b}:{label}")],"styles":{"color":"gold"}}]}),
    );
    // find_nodes sees the main and the instance layers, and says which is which.
    let found = edit(&mut s, "find_nodes", json!({"text":"Save"}));
    let rows = found["nodes"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{found}");
    assert!(rows.iter().any(|r| r["inInstance"] == json!(a)), "{found}");
    let roots = edit(&mut s, "find_nodes", json!({"tag":"button"}));
    assert!(
        roots["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["isComponent"] == json!(true))
    );
    assert!(
        roots["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["instanceOf"] == json!(main))
    );
    // A query restyling every white label hits the main and A's copy at once: that is a main
    // edit, so A keeps following the main and B keeps its own override.
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"query":{"styles":{"color":"white"}},"styles":{"color":"tomato"}}]}),
    );
    assert!(node(&s, &a).instance.as_ref().unwrap().overrides.is_empty());
    assert_eq!(node(&s, &format!("{a}:{label}")).styles["color"], "tomato");
    assert_eq!(node(&s, &format!("{b}:{label}")).styles["color"], "gold");
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[label],"styles":{"color":"navy"}}]}),
    );
    assert_eq!(node(&s, &format!("{a}:{label}")).styles["color"], "navy");
    assert_eq!(node(&s, &format!("{b}:{label}")).styles["color"], "gold");
    // A query edit that reaches only an instance layer is still an override.
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"query":{"nodeId":a,"text":"Save"},"styles":{"font-weight":"700"}}]}),
    );
    let overrides = &node(&s, &a).instance.as_ref().unwrap().overrides;
    assert_eq!(overrides[&label].styles["font-weight"], "700");
}

#[test]
fn bare_component_ids_and_undo_cover_instances() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let number = main.trim_start_matches("node-").parse::<u64>().unwrap();
    let a = edit(
        &mut s,
        "insert_instance",
        json!({"componentId":number,"parentId":frame}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(node(&s, &a).instance.as_ref().unwrap().component_id, main);
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":label,"textContent":"Send"}]}),
    );
    assert_eq!(node(&s, &format!("{a}:{label}")).text, "Send");
    // One undo reverts the main edit and what it did to the instances.
    edit(&mut s, "undo", json!({}));
    assert_eq!(node(&s, &label).text, "Save");
    assert_eq!(node(&s, &format!("{a}:{label}")).text, "Save");
    edit(&mut s, "redo", json!({}));
    assert_eq!(node(&s, &format!("{a}:{label}")).text, "Send");
}

#[test]
fn icon_layers_inside_components_follow_the_main() {
    let mut s = Store::memory();
    let frame = artboard(&mut s, "Screen");
    let main = insert(
        &mut s,
        &frame,
        "<div layer-name=\"Chip\"><svg data-icon=\"test:dot\" viewBox=\"0 0 24 24\" width=\"16\" height=\"16\"><circle cx=\"12\" cy=\"12\" r=\"4\" fill=\"currentColor\"/></svg><span>Live</span></div>",
    )[0]
    .clone();
    let icon = node(&s, &main).children[0].id.clone();
    assert!(node(&s, &icon).markup.is_some());
    edit(&mut s, "create_component", json!({"nodeId":main}));
    let a = instance(&mut s, &main, &frame);
    let inner = format!("{a}:{icon}");
    assert_eq!(node(&s, &inner).markup, node(&s, &icon).markup);
    // Recoloring the instance's icon is an override; the icon itself stays the main's.
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[inner],"styles":{"color":"red"}}]}),
    );
    let found = edit(&mut s, "find_nodes", json!({"tag":"icon","nodeId":a}));
    assert_eq!(found["nodes"][0]["icon"], "test:dot");
    assert_eq!(found["nodes"][0]["inInstance"], json!(a));
    let html = html::nodes_html(doc(&s), vec![node(&s, &a)], false);
    assert!(html.contains("<circle"), "{html}");
}

#[test]
fn instance_root_answers_to_its_layer_style_id() {
    let mut s = Store::memory();
    let (frame, main, label) = button(&mut s);
    let a = instance(&mut s, &main, &frame);
    // Inner layers are a:<layer>, so agents guess a:<main> for the root; it means the root.
    let alias = format!("{a}:{main}");
    edit(
        &mut s,
        "batch",
        json!({"operations":[
            {"tool":"update_styles","args":{"updates":[{"nodeIds":[alias],"styles":{"background":"#eee"}}]}},
            {"tool":"set_text_content","args":{"updates":[{"nodeId":format!("{a}:{label}"),"textContent":"Cancel"}]}},
            {"tool":"rename_nodes","args":{"updates":[{"nodeId":alias,"name":"Cancel button"}]}}
        ]}),
    );
    assert_eq!(node(&s, &a).styles["background"], "#eee");
    assert_eq!(node(&s, &a).name, "Cancel button");
    let info = edit(&mut s, "get_node_info", json!({"nodeId":alias}));
    assert_eq!(info["id"], json!(a));
    // A wrong inner id says how instance layers are named.
    let err = tools::call(
        &mut s,
        "update_styles",
        &json!({"updates":[{"nodeIds":[format!("{a}:node-999")],"styles":{"color":"red"}}]}),
    )
    .unwrap_err();
    assert!(err.contains(&format!("its root is {a}")), "{err}");
}

#[test]
fn create_component_can_park_the_main_on_a_components_page() {
    let mut s = Store::memory();
    let frame = artboard(&mut s, "Landing");
    let ids = insert(
        &mut s,
        &frame,
        "<h1>Title</h1><div layer-name=\"Feature\" style=\"display:flex\"><span>Fast</span></div><p>Footer</p>",
    );
    let (row, label) = (ids[1].clone(), node(&s, &ids[1]).children[0].id.clone());
    let pages = doc(&s).pages.len();
    let made = edit(
        &mut s,
        "create_component",
        json!({"nodeId":row,"moveToPage":"Components"}),
    );
    let instance = made["instanceId"].as_str().unwrap().to_string();
    let page = made["pageId"].as_str().unwrap().to_string();
    assert_eq!(made["componentId"], json!(row));
    let d = doc(&s);
    assert_eq!(d.pages.len(), pages + 1);
    let components = d.pages.iter().find(|p| p.id == page).unwrap();
    assert_eq!(components.name, "Components");
    assert_eq!(components.artboards[0].id, row);
    assert!(components.artboards[0].component && components.artboards[0].position.is_some());
    // The instance sits exactly where the layer was and still follows the main.
    let kids: Vec<_> = node(&s, &frame)
        .children
        .iter()
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(kids, [ids[0].clone(), instance.clone(), ids[2].clone()]);
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":label,"textContent":"Faster"}]}),
    );
    assert_eq!(node(&s, &format!("{instance}:{label}")).text, "Faster");

    // A second component lands on the same page by name, beside the first.
    let other = insert(&mut s, &frame, "<div layer-name=\"Badge\">New</div>")[0].clone();
    let second = edit(
        &mut s,
        "create_component",
        json!({"nodeId":other,"moveToPage":"components"}),
    );
    assert_eq!(second["pageId"], json!(page));
    let d = doc(&s);
    let components = d.pages.iter().find(|p| p.id == page).unwrap();
    assert_eq!(components.artboards.len(), 2);
    assert!(
        components.artboards[1].position.unwrap()[0] > components.artboards[0].position.unwrap()[0]
    );

    // Each create_component is one undo step: undo the badge, its insert and the text edit, then
    // one more undo puts the row back in place, unlinked, and drops the new page.
    for _ in 0..3 {
        edit(&mut s, "undo", json!({}));
    }
    assert!(doc(&s).pages.iter().any(|p| p.id == page));
    edit(&mut s, "undo", json!({}));
    let d = doc(&s);
    assert_eq!(d.pages.len(), pages);
    assert_eq!(node(&s, &frame).children[1].id, row);
    assert!(!node(&s, &row).component && d.node(&instance).is_err());

    let err = tools::call(
        &mut s,
        "create_component",
        &json!({"nodeId":row,"moveToPage":"page-999"}),
    )
    .unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
    assert!(!node(&s, &row).component);
}
