//! Icons without network: every test reads from a seeded disk cache.
use cardboard::{document::Store, html, icons, tools};
use serde_json::{Value, json};

const ARROW: &str = r#"<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14m-7-7l7 7l-7 7"/>"#;
const STAR: &str = r#"<path fill="currentColor" d="M12 2l3 7h7l-6 4l2 7l-6-4l-6 4l2-7l-6-4h7z"/>"#;

fn seed() {
    let dir = std::env::temp_dir().join(format!("cardboard-icons-test-{}", std::process::id()));
    icons::set_cache_dir(dir);
    let icon = |body: &str, w: f64| icons::Icon {
        body: body.into(),
        view_box: [0.0, 0.0, w, 24.0],
    };
    icons::cache_icon("lucide:arrow-right", &icon(ARROW, 24.0)).unwrap();
    icons::cache_icon("mdi:star", &icon(STAR, 24.0)).unwrap();
    icons::cache_icon("logos:wide", &icon(STAR, 48.0)).unwrap();
}
fn edit(s: &mut Store, name: &str, args: Value) -> Value {
    tools::call(s, name, &args).unwrap()
}
fn setup() -> (Store, String) {
    seed();
    let mut s = Store::memory();
    let frame = edit(
        &mut s,
        "create_artboard",
        json!({"name":"Desktop","styles":{"width":"400px","height":"300px"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    (s, frame)
}

#[test]
fn insert_icon_is_one_layer_that_exports_and_undoes() {
    let (mut s, frame) = setup();
    let id = edit(
        &mut s,
        "insert_icon",
        json!({"icon":"lucide:arrow-right","targetNodeId":frame,"size":20,"color":"var(--accent)"}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let file = s.active_file.clone().unwrap();
    let doc = s.file(&file).unwrap();
    let node = doc.node(&id).unwrap();
    assert_eq!(
        (node.tag.as_str(), node.name.as_str()),
        ("svg", "arrow-right")
    );
    assert!(node.children.is_empty());
    assert_eq!(node.styles["width"], "20px");
    assert_eq!(node.styles["color"], "var(--accent)");
    assert_eq!(node.attributes["data-icon"], "lucide:arrow-right");

    let html = html::document_html(doc, false);
    assert!(html.contains(r#"viewBox="0 0 24 24""#));
    assert!(html.contains(r#"stroke="currentColor""#));
    assert!(html.contains(r#"d="M5 12h14m-7-7l7 7l-7 7"/></svg>"#));
    let jsx = edit(&mut s, "get_jsx", json!({"nodeId":id}))["jsx"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(jsx.contains(r#"strokeWidth={"2"}"#) && jsx.contains(r#"strokeLinecap={"round"}"#));
    let info = edit(&mut s, "get_node_info", json!({"nodeId":id}));
    assert_eq!(info["icon"], "lucide:arrow-right");

    // Recolor and resize like any layer.
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[id],"styles":{"color":"red","width":"32px","height":"32px"}}]}),
    );
    edit(&mut s, "undo", json!({}));
    edit(&mut s, "undo", json!({}));
    assert!(s.file(&file).unwrap().node(&id).is_err());
}

#[test]
fn write_html_icon_element_sits_inline_with_text() {
    let (mut s, frame) = setup();
    let ids = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":
            r#"<button style="display:flex;gap:8px">Next<icon name="lucide:arrow-right" size="16" stroke-width="1.5" style="margin-left:4px" layer-name="Next icon" /></button>"#}),
    )["nodeIds"]
        .clone();
    let doc = s.file(&s.active_file.clone().unwrap()).unwrap();
    let button = doc.node(ids[0].as_str().unwrap()).unwrap();
    assert_eq!(button.children.len(), 2);
    let icon = &button.children[1];
    assert_eq!(icon.name, "Next icon");
    assert_eq!(icon.styles["height"], "16px");
    assert_eq!(icon.styles["margin-left"], "4px");
    assert!(
        icon.markup
            .as_ref()
            .unwrap()
            .contains(r#"stroke-width="1.5""#)
    );
}

#[test]
fn non_square_icons_keep_their_aspect_ratio() {
    let (mut s, frame) = setup();
    let id = edit(
        &mut s,
        "insert_icon",
        json!({"icon":"logos:wide","targetNodeId":frame,"size":"10"}),
    )["nodeId"]
        .clone();
    let doc = s.file(&s.active_file.clone().unwrap()).unwrap();
    let node = doc.node(id.as_str().unwrap()).unwrap();
    assert_eq!(
        (
            node.styles["width"].as_str(),
            node.styles["height"].as_str()
        ),
        ("20px", "10px")
    );
}

#[test]
fn unusable_icon_requests_fail_clearly_without_changes() {
    let (mut s, frame) = setup();
    let fail = |s: &mut Store, args: Value| tools::call(s, "insert_icon", &args).unwrap_err();
    assert!(
        fail(
            &mut s,
            json!({"icon":"lucide:not-cached","targetNodeId":frame})
        )
        .contains("not cached")
    );
    assert!(
        fail(&mut s, json!({"icon":"../etc/passwd","targetNodeId":frame})).contains("set:name")
    );
    assert!(
        fail(
            &mut s,
            json!({"icon":"mdi:star","targetNodeId":frame,"strokeWidth":1})
        )
        .contains("filled icon")
    );
    assert!(
        tools::call(
            &mut s,
            "write_html",
            &json!({"targetNodeId":frame,"mode":"insert-children","html":"<icon name=\"mdi:star\">label</icon>"}),
        )
        .is_err()
    );
    let doc = s.file(&s.active_file.clone().unwrap()).unwrap();
    assert!(doc.node(&frame).unwrap().children.is_empty());
}

#[test]
fn sanitizer_rejects_active_or_external_content() {
    for bad in [
        r#"<script>alert(1)</script>"#,
        r#"<foreignObject><div/></foreignObject>"#,
        r#"<path onload="alert(1)" d="M0 0"/>"#,
        r#"<use href="https://evil.example/x.svg#a"/>"#,
        r#"<use xlink:href="javascript:alert(1)"/>"#,
        r#"<path fill="url(https://evil.example/a)" d="M0 0"/>"#,
        r#"<path style="fill:url('http://x/a')" d="M0 0"/>"#,
        r#"<set attributeName="href" to="javascript:alert(1)"/>"#,
        r#"<image href="x.png"/>"#,
        r#"<text>hi</text>"#,
    ] {
        assert!(icons::sanitize(bad).is_err(), "accepted {bad}");
    }
    let ok = r##"<!-- c --><title>Star</title><defs><linearGradient id="g"><stop offset="0" stop-color="red"/></linearGradient></defs><path fill="url(#g)" d="M0 0"/><use href="#g"/>"##;
    let clean = icons::sanitize(ok).unwrap();
    assert_eq!(icons::sanitize(&clean).unwrap(), clean);
    assert!(!clean.contains("title") && !clean.contains("<!--"));
}

#[test]
fn stored_markup_must_be_sanitized_to_load() {
    let (mut s, frame) = setup();
    let id = edit(
        &mut s,
        "insert_icon",
        json!({"icon":"mdi:star","targetNodeId":frame}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let file = s.active_file.clone().unwrap();
    let doc = &mut s.files.get_mut(&file).unwrap().doc;
    assert!(doc.validate().is_ok());
    doc.node_mut(&id).unwrap().markup = Some("<path d=\"M0 0\"/><script>alert(1)</script>".into());
    assert!(doc.validate().is_err());
    doc.node_mut(&id).unwrap().markup = Some("<path d='M0 0'/>".into());
    assert!(doc.validate().is_err(), "non-canonical markup is rejected");
    // Icons hold no children or text.
    doc.node_mut(&id).unwrap().markup = Some(icons::sanitize(STAR).unwrap());
    assert!(doc.validate().is_ok());
    assert!(
        tools::call(
            &mut s,
            "set_text_content",
            &json!({"updates":[{"nodeId":id,"textContent":"x"}]})
        )
        .is_err()
    );
}

#[test]
fn icon_names_are_collected_before_a_call_runs() {
    let names = icons::referenced(
        "batch",
        &json!({"operations":[
            {"tool":"insert_icon","args":{"icon":"lucide:x"}},
            {"tool":"write_html","args":{"html":"<div><icon name=\"ph:house\" /><icon name='tabler:a' /></div>"}},
        ]}),
    );
    assert_eq!(names, ["lucide:x", "ph:house", "tabler:a"]);
}

#[test]
fn exported_icon_html_imports_back_as_one_layer() {
    let (mut s, frame) = setup();
    let id = edit(
        &mut s,
        "insert_icon",
        json!({"icon":"lucide:arrow-right","targetNodeId":frame,"strokeWidth":3}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let file = s.active_file.clone().unwrap();
    let original = s.file(&file).unwrap().node(&id).unwrap().clone();
    let exported = html::node_html(&original, false);
    let ids = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":exported}),
    )["nodeIds"]
        .clone();
    let copy = s
        .file(&file)
        .unwrap()
        .node(ids[0].as_str().unwrap())
        .unwrap();
    assert!(copy.children.is_empty());
    assert_eq!(copy.markup, original.markup);
    assert_eq!(copy.styles, original.styles);
    assert_eq!(copy.attributes, original.attributes);
}

#[test]
fn icons_work_in_browser_style_html() {
    let (mut s, frame) = setup();
    // Entities, an unclosed void and an unquoted attribute that a strict XML reader would stop at.
    let markup = "<p>Next&nbsp;step<br></p><button class=cta>Go <icon name=\"lucide:arrow-right\" size=16></icon></button>";
    assert_eq!(
        icons::referenced("write_html", &json!({ "html": markup })),
        ["lucide:arrow-right"]
    );
    let ids = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":markup}),
    )["nodeIds"]
        .clone();
    let doc = s.file(&s.active_file.clone().unwrap()).unwrap();
    let button = doc.node(ids[1].as_str().unwrap()).unwrap();
    assert_eq!(button.children.len(), 2);
    assert_eq!(button.children[0].text, "Go ");
    let icon = &button.children[1];
    assert_eq!(icon.tag, "svg");
    assert!(icon.markup.is_some() && icon.children.is_empty());
    assert_eq!(icon.styles["height"], "16px");
}

#[test]
fn find_nodes_and_claims_understand_icon_layers() {
    let (mut s, frame) = setup();
    let agent = |id: &str| cardboard::presence::Actor {
        id: id.into(),
        name: format!("agent {id}"),
    };
    s.actor = Some(agent("a"));
    let icon = edit(
        &mut s,
        "insert_icon",
        json!({"icon":"lucide:arrow-right","targetNodeId":frame}),
    )["nodeId"]
        .clone();
    s.actor = Some(agent("b"));
    let blocked = tools::call(
        &mut s,
        "update_styles",
        &json!({"updates":[{"nodeIds":[icon],"styles":{"color":"red"}}]}),
    );
    assert!(blocked.unwrap_err().contains("agent a"));
    s.actor = None;
    for query in [json!({"tag":"icon"}), json!({"name":"lucide:arrow"})] {
        let found = edit(&mut s, "find_nodes", query);
        assert_eq!(found["nodes"][0]["id"], icon, "{found}");
        assert_eq!(found["nodes"][0]["icon"], "lucide:arrow-right");
    }
    // Path data is not text: searching it finds nothing.
    assert_eq!(edit(&mut s, "find_nodes", json!({"text":"M5"}))["total"], 0);
}
