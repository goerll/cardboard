use cardboard::{document::Store, html, store, tools};
use serde_json::{Value, json};

fn edit(s: &mut Store, name: &str, args: Value) -> Value {
    tools::call(s, name, &args).unwrap()
}
fn fid(s: &Store) -> String {
    s.active_file.clone().unwrap()
}

#[test]
fn imported_heading_entities_remain_one_editable_text_layer() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let heading = insert(&mut s, &frame, "<h1>Hello &amp; goodbye</h1>")[0].clone();
    let node = s.file(&fid(&s)).unwrap().node(&heading).unwrap();
    assert_eq!(node.text, "Hello & goodbye");
    assert!(node.children.is_empty());
    edit(
        &mut s,
        "set_text_content",
        json!({"updates":[{"nodeId":heading,"textContent":"New heading"}]}),
    );
    assert!(html::document_html(s.file(&fid(&s)).unwrap(), false).contains(">New heading</h1>"));
}

#[test]
fn creating_a_page_in_an_empty_file_and_undoing_it_succeed() {
    let mut s = Store::memory();
    let first = fid(&s);
    let page = edit(&mut s, "create_page", json!({"name":"Second"}));
    assert_ne!(page["pageId"], "page-1");
    let file = edit(&mut s, "create_file", json!({"name":"Another"}));
    assert_eq!(s.active_file.as_deref(), file["fileId"].as_str());
    assert!(tools::call(&mut s, "undo", &json!({})).is_err());
    edit(&mut s, "open_file", json!({"fileId":first}));
    edit(&mut s, "undo", json!({}));
    assert_eq!(s.file(&first).unwrap().pages.len(), 1);
    edit(&mut s, "redo", json!({}));
    assert_eq!(s.file(&first).unwrap().pages.len(), 2);
    assert_eq!(s.files.len(), 2);
}
#[test]
fn removing_a_style_changes_export_and_invalid_id_counters_are_rejected() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[frame],"styles":{"backgroundColor":"red"}}]}),
    );
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[frame],"styles":{"backgroundColor":""}}]}),
    );
    let id = fid(&s);
    let doc = &mut s.files.get_mut(&id).unwrap().doc;
    assert!(
        !doc.node(&frame)
            .unwrap()
            .styles
            .contains_key("background-color")
    );
    assert!(!html::document_html(doc, false).contains("background-color:red"));
    doc.next_id = 1;
    assert!(doc.validate().is_err());
}
fn artboard(s: &mut Store) -> String {
    edit(
        s,
        "create_artboard",
        json!({"name":"Desktop","styles":{"width":"1440px","height":"900px"}}),
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

#[test]
fn atomic_batch_rolls_back_all_changes_on_missing_node() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let revision = s.file(&fid(&s)).unwrap().revision;
    let result = tools::call(
        &mut s,
        "update_styles",
        &json!({"updates":[{"nodeIds":[frame,"missing"],"styles":{"backgroundColor":"red"}}]}),
    );
    assert!(result.is_err());
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.revision, revision);
    assert!(
        !doc.node(&frame)
            .unwrap()
            .styles
            .contains_key("background-color")
    );
}
#[test]
fn reparenting_cannot_create_cycles_and_preserves_ids() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let child = insert(&mut s, &frame, "<div><p>Hello</p></div>")[0].clone();
    let before = s.file(&fid(&s)).unwrap().clone();
    assert!(
        tools::call(
            &mut s,
            "move_nodes",
            &json!({"moves":[{"nodeId":frame,"parentId":child}]})
        )
        .is_err()
    );
    assert_eq!(*s.file(&fid(&s)).unwrap(), before);
    edit(
        &mut s,
        "move_nodes",
        json!({"moves":[{"nodeId":child,"parentId":"root","index":0}]}),
    );
    assert_eq!(s.file(&fid(&s)).unwrap().pages[0].artboards[0].id, child);
}
#[test]
fn duplicate_rekeys_every_descendant() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    insert(&mut s, &frame, "<div><span>Original</span></div>");
    let copy =
        edit(&mut s, "duplicate_nodes", json!({"nodes":[{"id":frame}]}))["duplicates"][0]["nodeId"]
            .as_str()
            .unwrap()
            .to_owned();
    let d = s.file(&fid(&s)).unwrap();
    let a = d.node(&frame).unwrap();
    let b = d.node(&copy).unwrap();
    assert_ne!(a.id, b.id);
    assert_ne!(a.children[0].id, b.children[0].id);
    assert_eq!(a.count(), b.count());
    d.validate().unwrap();
}
#[test]
fn undo_redo_preserves_monotonic_revisions_and_never_reuses_ids() {
    let mut s = Store::memory();
    let a = artboard(&mut s);
    let r = s.file(&fid(&s)).unwrap().revision;
    {
        let id = fid(&s);
        s.undo(&id).unwrap()
    };
    assert!(s.file(&fid(&s)).unwrap().revision > r);
    {
        let id = fid(&s);
        s.redo(&id).unwrap()
    };
    assert!(s.file(&fid(&s)).unwrap().node(&a).is_ok());
    {
        let id = fid(&s);
        s.undo(&id).unwrap()
    };
    let b = artboard(&mut s);
    assert_ne!(a, b);
    assert!(s.redo(&fid(&s)).is_err());
}
#[test]
fn explicit_file_target_is_independent_of_active_tab() {
    let mut s = Store::memory();
    let one = fid(&s);
    let first = artboard(&mut s);
    let second = edit(&mut s, "create_file", json!({"name":"Other"}))["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    edit(&mut s, "open_file", json!({"fileId":second}));
    edit(
        &mut s,
        "update_styles",
        json!({"fileId":one,"updates":[{"nodeIds":[first],"styles":{"color":"red"}}]}),
    );
    assert_eq!(s.active_file.as_ref(), Some(&second));
    assert_eq!(
        s.file(&one).unwrap().node(&first).unwrap().styles["color"],
        "red"
    );
    assert!(
        tools::call(
            &mut s,
            "get_node_info",
            &json!({"fileId":second,"nodeId":first})
        )
        .is_err()
    );
}
#[test]
fn stale_revision_rejects_an_edit_without_changing_history() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    assert!(
        tools::call(
            &mut s,
            "update_styles",
            &json!({"expectedRevision":0,"updates":[{"nodeIds":[frame],"styles":{"color":"red"}}]})
        )
        .is_err()
    );
    assert_eq!(s.file(&fid(&s)).unwrap().revision, 1);
    {
        let id = fid(&s);
        s.undo(&id).unwrap()
    };
    assert!(s.file(&fid(&s)).unwrap().pages[0].artboards.is_empty());
}
#[test]
fn import_handles_entities_void_elements_and_css_function_semicolons() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    insert(
        &mut s,
        &frame,
        "<div layer-name=\"Photo\" style=\"background-image:url('data:image/png;base64,AAAA');padding:16px\"><img src=\"https://example.com/a.png\" />Hello &amp; goodbye</div>",
    );
    let doc = s.file(&fid(&s)).unwrap();
    let result = html::document_html(doc, false);
    assert!(result.contains("Hello "));
    assert!(result.contains("&amp;"));
    assert!(!result.contains("data-cardboard-id"));
    assert!(result.contains("data:image/png;base64,AAAA"));
}
#[test]
fn import_keeps_svg_text_attributes() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    insert(
        &mut s,
        &frame,
        "<svg viewBox=\"0 0 100 20\" xmlns=\"http://www.w3.org/2000/svg\"><text x=\"50\" y=\"14\" text-anchor=\"middle\" font-family=\"Inter\" font-size=\"12\" font-weight=\"600\">Label</text></svg>",
    );
    let result = html::document_html(s.file(&fid(&s)).unwrap(), false);
    assert!(result.contains("text-anchor=\"middle\""));
    assert!(result.contains("font-weight=\"600\""));
}
#[test]
fn invalid_html_does_not_consume_ids_or_leave_partial_nodes() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let before = s.file(&fid(&s)).unwrap().clone();
    assert!(
        tools::call(
            &mut s,
            "write_html",
            &json!({"html":"<div><span>unfinished","targetNodeId":frame,"mode":"insert-children"})
        )
        .is_err()
    );
    assert_eq!(*s.file(&fid(&s)).unwrap(), before);
}
#[test]
fn imported_designs_cannot_execute_scripts_or_break_style_blocks() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    for fragment in [
        "<script>alert(1)</script>",
        "<div onclick=\"alert(1)\"></div>",
        "<a href=\"javascript:alert(1)\">Link</a>",
        "<iframe srcdoc=\"bad\"></iframe>",
    ] {
        assert!(
            tools::call(
                &mut s,
                "write_html",
                &json!({"html":fragment,"targetNodeId":frame,"mode":"insert-children"})
            )
            .is_err()
        );
    }
    assert!(
        tools::call(
            &mut s,
            "create_tokens",
            &json!({"tokens":[{"name":"--bad","value":"</style><script>bad</script>"}]})
        )
        .is_err()
    );
    assert!(html::parse_styles("color: red; background: black").is_ok());
    assert!(html::validate_style("color", "red;background:black").is_err());
}
#[test]
fn token_rename_updates_aliases_and_authored_references() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    edit(
        &mut s,
        "create_tokens",
        json!({"tokens":[{"name":"--color-brand","value":"#326bfa"},{"name":"--color-link","value":"var(--color-brand)"}]}),
    );
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[frame],"styles":{"color":"var(--color-brand)"}}]}),
    );
    edit(
        &mut s,
        "set_tokens",
        json!({"tokens":[{"name":"--color-brand","newName":"--color-primary"}]}),
    );
    let d = s.file(&fid(&s)).unwrap();
    assert_eq!(
        d.node(&frame).unwrap().styles["color"],
        "var(--color-primary)"
    );
    assert_eq!(d.tokens["--color-link"], "var(--color-primary)");
    assert!(
        tools::call(
            &mut s,
            "create_tokens",
            &json!({"tokens":[{"name":"--color-primary","value":"red"}]})
        )
        .is_err()
    );
}
#[test]
fn deleting_an_ancestor_and_descendant_in_one_batch_succeeds() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let child = insert(&mut s, &frame, "<div>Child</div>")[0].clone();
    edit(&mut s, "delete_nodes", json!({"nodeIds":[frame,child]}));
    assert!(s.file(&fid(&s)).unwrap().pages[0].artboards.is_empty());
    {
        let id = fid(&s);
        s.undo(&id).unwrap()
    };
    assert!(s.file(&fid(&s)).unwrap().node(&child).is_ok());
}
#[test]
fn save_failure_rolls_back_edit_and_history() {
    let dir = temp_dir("save-failure");
    let path = dir.join("design.cardboard");
    let mut s = Store::open_document(&path).unwrap();
    let frame = artboard(&mut s);
    let before = s.file(&fid(&s)).unwrap().clone();
    // A directory cannot be replaced by the atomic file rename.
    let id = fid(&s);
    s.files.get_mut(&id).unwrap().path = Some(dir.clone());
    assert!(
        tools::call(
            &mut s,
            "update_styles",
            &json!({"updates":[{"nodeIds":[frame],"styles":{"color":"red"}}]})
        )
        .is_err()
    );
    assert_eq!(*s.file(&id).unwrap(), before);
    s.files.get_mut(&id).unwrap().path = Some(path);
    s.undo(&id).unwrap();
    assert!(s.file(&id).unwrap().pages[0].artboards.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn saved_file_reopens_with_export_fidelity() {
    let dir = temp_dir("reopen");
    let path = dir.join("design.cardboard");
    let mut s = Store::open_document(&path).unwrap();
    let frame = artboard(&mut s);
    insert(
        &mut s,
        &frame,
        "<h1 style=\"font-size:32px;color:oklch(60% 0.2 240)\">Hello</h1>",
    );
    let reopened = Store::open_document(&path).unwrap();
    assert_eq!(
        html::document_html(s.file(&fid(&s)).unwrap(), false),
        html::document_html(reopened.file(&fid(&reopened)).unwrap(), false)
    );
    let raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["format"], "cardboard");
    assert_eq!(raw["version"], 2);
    std::fs::remove_dir_all(dir).unwrap();
}
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cardboard-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn batch_is_one_atomic_undo_step() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let id = fid(&s);
    let revision = s.file(&id).unwrap().revision;
    let result = edit(
        &mut s,
        "batch",
        json!({"operations":[
            {"tool":"write_html","args":{"html":"<p layer-name=\"Intro\">Hi</p>","targetNodeId":frame,"mode":"insert-children"}},
            {"tool":"update_styles","args":{"updates":[{"nodeIds":[frame],"styles":{"gap":"8px"}}]}},
        ]}),
    );
    let child = result["results"][0]["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(s.file(&id).unwrap().node(&child).unwrap().name, "Intro");
    assert_eq!(s.file(&id).unwrap().revision, revision + 1);
    s.undo(&id).unwrap();
    let doc = s.file(&id).unwrap();
    assert!(doc.node(&child).is_err() && !doc.node(&frame).unwrap().styles.contains_key("gap"));
    // A failing operation rolls back the earlier ones.
    let before = s.file(&id).unwrap().clone();
    assert!(
        tools::call(
            &mut s,
            "batch",
            &json!({"operations":[
                {"tool":"update_styles","args":{"updates":[{"nodeIds":[frame],"styles":{"gap":"8px"}}]}},
                {"tool":"delete_nodes","args":{"nodeIds":["missing"]}},
            ]})
        )
        .is_err()
    );
    assert_eq!(*s.file(&id).unwrap(), before);
    assert!(tools::call(&mut s, "batch", &json!({"operations":[{"tool":"undo"}]})).is_err());
}
#[test]
fn selection_and_page_changes_skip_history_and_revisions() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let id = fid(&s);
    let (revision, generation) = (s.file(&id).unwrap().revision, s.generation);
    let seq = s.files[&id].seq;
    edit(&mut s, "select_nodes", json!({"nodeIds":[frame]}));
    assert_eq!(s.file(&id).unwrap().selection, vec![frame.clone()]);
    assert_eq!(s.file(&id).unwrap().revision, revision);
    assert!(s.generation > generation && s.files[&id].seq > seq);
    let page = edit(&mut s, "create_page", json!({"name":"Two"}))["pageId"]
        .as_str()
        .unwrap()
        .to_owned();
    edit(&mut s, "set_active_page", json!({"pageId":page}));
    assert!(tools::call(&mut s, "select_nodes", &json!({"nodeIds":["missing"]})).is_err());
    // Undo reverts the page creation, not the page switch; the view falls back to a real page.
    s.undo(&id).unwrap();
    let doc = s.file(&id).unwrap();
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.active_page, "page-1");
}
#[test]
fn page_background_and_positions_round_trip() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let id = fid(&s);
    edit(
        &mut s,
        "update_pages",
        json!({"updates":[{"pageId":"page-1","background":"#101010","name":"Main"}]}),
    );
    assert!(
        tools::call(
            &mut s,
            "update_pages",
            &json!({"updates":[{"pageId":"page-1","background":"red;x:y"}]})
        )
        .is_err()
    );
    edit(
        &mut s,
        "set_positions",
        json!({"updates":[{"nodeId":frame,"x":120,"y":-40}]}),
    );
    let doc = s.file(&id).unwrap();
    assert_eq!(doc.pages[0].background.as_deref(), Some("#101010"));
    assert_eq!(doc.pages[0].artboards[0].position, Some([120.0, -40.0]));
    let child = insert(&mut s, &frame, "<div>a</div>")[0].clone();
    assert!(
        tools::call(
            &mut s,
            "set_positions",
            &json!({"updates":[{"nodeId":child,"x":1,"y":1}]})
        )
        .is_err()
    );
    let second = insert(&mut s, &frame, "<p>b</p>")[0].clone();
    let first = edit(
        &mut s,
        "write_html",
        json!({"html":"<span>c</span>","targetNodeId":frame,"mode":"insert-children","index":0}),
    )["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let kids: Vec<_> = s
        .file(&id)
        .unwrap()
        .node(&frame)
        .unwrap()
        .children
        .iter()
        .map(|n| n.id.clone())
        .collect();
    assert_eq!(kids, vec![first, child, second]);
}
#[test]
fn library_files_archive_restore_and_legacy_import() {
    let dir = temp_dir("library");
    let legacy = dir.join("workspace.json");
    let mut doc = cardboard::document::Document::new("file-1".into(), "Old design");
    doc.revision = 3;
    std::fs::write(
        &legacy,
        serde_json::to_vec(
            &json!({"formatVersion":1,"nextFile":2,"activeFile":"file-1","files":{"file-1":doc}}),
        )
        .unwrap(),
    )
    .unwrap();
    let library = dir.join("lib");
    let mut s = Store::library(&library, Some(&legacy)).unwrap();
    let files = edit(&mut s, "list_files", json!({}))["files"].clone();
    assert_eq!(files.as_array().unwrap().len(), 1);
    assert_eq!(files[0]["name"], "Old design");
    let created = edit(&mut s, "create_file", json!({"name":"New one"}));
    let path = created["path"].as_str().unwrap().to_owned();
    assert!(path.ends_with("New one.cardboard"));
    artboard(&mut s);
    s.flush().unwrap();
    edit(&mut s, "rename_file", json!({"name":"Renamed"}));
    let path = s.path_of(&fid(&s)).unwrap().clone();
    assert!(path.ends_with("Renamed.cardboard"));
    let archived = edit(&mut s, "archive_file", json!({"path":path}))["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(s.files.is_empty() && s.active_file.is_none());
    assert_eq!(
        edit(&mut s, "list_files", json!({"archived":true}))["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(tools::call(&mut s, "delete_file", &json!({"path":files[0]["path"]})).is_err());
    let restored = edit(&mut s, "restore_file", json!({"path":archived}))["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let opened = edit(&mut s, "open_file", json!({"path":restored}));
    assert_eq!(
        s.file(opened["fileId"].as_str().unwrap()).unwrap().pages[0]
            .artboards
            .len(),
        1
    );
    assert!(tools::call(&mut s, "open_file", &json!({"path":legacy})).is_err());
    assert!(tools::call(&mut s, "archive_file", &json!({"path":"/etc/passwd"})).is_err());
    s.flush().unwrap();
    // Tabs come back; the legacy workspace is not imported twice.
    let again = Store::library(&library, Some(&legacy)).unwrap();
    assert_eq!(again.tabs.len(), 1);
    assert_eq!(again.scan(false).len(), 2);
    let _ = store::EXTENSION;
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn public_tool_schemas_are_valid_sdk_tools_and_unique() {
    let definitions = tools::definitions();
    let mut names = std::collections::BTreeSet::new();
    for def in definitions {
        assert!(names.insert(def["name"].as_str().unwrap().to_owned()));
        serde_json::from_value::<rmcp::model::Tool>(def).unwrap();
    }
    assert!(
        names.contains("write_html") && names.contains("batch") && names.contains("list_files")
    );
    assert!(!names.contains("get_computed_styles"));
    assert!(!names.contains("get_screenshot"));
}

#[test]
fn alt_drag_duplicate_places_copies_in_one_undo_step() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let ids = insert(&mut s, &frame, "<div>a</div><div>b</div>");
    let copy = edit(
        &mut s,
        "duplicate_nodes",
        json!({"nodes":[{"id":ids[0],"parentId":frame,"index":2,"styles":{"color":"red"}}]}),
    )["duplicates"][0]["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let doc = s.file(&fid(&s)).unwrap();
    let children: Vec<_> = doc
        .node(&frame)
        .unwrap()
        .children
        .iter()
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(children, vec![ids[0].clone(), ids[1].clone(), copy.clone()]);
    assert_eq!(
        doc.node(&copy)
            .unwrap()
            .styles
            .get("color")
            .map(String::as_str),
        Some("red")
    );

    let ab = edit(
        &mut s,
        "duplicate_nodes",
        json!({"nodes":[{"id":frame,"x":2000,"y":50}]}),
    )["duplicates"][0]["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        s.file(&fid(&s)).unwrap().node(&ab).unwrap().position,
        Some([2000.0, 50.0])
    );
    edit(&mut s, "undo", json!({}));
    assert!(s.file(&fid(&s)).unwrap().node(&ab).is_err());
}
fn write(s: &mut Store, args: Value) -> Result<Value, String> {
    tools::call(s, "write_html", &args)
}
fn tags(node: &cardboard::document::Node) -> Vec<String> {
    node.children.iter().map(|n| n.tag.clone()).collect()
}
#[test]
fn real_world_html_imports_like_a_browser() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let result = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":concat!(
            "<p>Paper & Ink &mdash; est.&nbsp;2026 &copy 2026</p>",
            "<ul><li>One<li>Two</ul>",
            "<div><img src=\"a.png\" alt=Cover><br><span>after</span></div>",
            "<dl><dt>Term<dd>Definition</dl>",
            "<figure><picture><source srcset=\"a.avif\"><img src=\"a.png\"></picture><figcaption>Cap</figcaption></figure>",
            "<div style=\"height:8px\" /><span>sibling</span>",
            "<label for=email>Email</label><input id=email disabled>",
        )}),
    );
    assert!(result.get("warnings").is_none(), "{result}");
    let doc = s.file(&fid(&s)).unwrap();
    let ids: Vec<&str> = result["nodeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let node = |i: usize| doc.node(ids[i]).unwrap();
    assert_eq!(
        node(0).text,
        "Paper & Ink \u{2014} est.\u{a0}2026 \u{a9} 2026"
    );
    assert_eq!(tags(node(1)), ["li", "li"]);
    assert_eq!(node(1).children[1].text, "Two");
    assert_eq!(tags(node(2)), ["img", "br", "span"]);
    assert_eq!(tags(node(3)), ["dt", "dd"]);
    assert_eq!(tags(node(4)), ["picture", "figcaption"]);
    // XML-style self-closing still means an empty element, not an open one.
    assert!(node(5).children.is_empty());
    assert_eq!(node(6).text, "sibling");
    assert_eq!(
        node(8).attributes.get("disabled").map(String::as_str),
        Some("")
    );
}
#[test]
fn unsupported_markup_maps_or_drops_with_warnings() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let result = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":concat!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Page</title><style>.a{color:red}</style></head><body>",
            "<my-card data-x=1><b>Bold</b></my-card>",
            "<svg viewBox=\"0 0 1 1\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><animate attributeName=\"x\"/><rect/></svg>",
            "<div itemscope style=\"color:red;background:url(javascript:alert(1))\">x</div>",
            "</body></html>",
        )}),
    );
    let warnings = result["warnings"].as_array().unwrap();
    let text = serde_json::to_string(warnings).unwrap();
    assert!(
        !text.contains("<meta>") && !text.contains("<title>"),
        "{text}"
    );
    for expected in [
        "<style>",
        "<my-card> is not supported; imported as <div>",
        "<animate>",
        "itemscope",
        "background",
    ] {
        assert!(text.contains(expected), "missing {expected} in {text}");
    }
    let doc = s.file(&fid(&s)).unwrap();
    let card = doc.node(result["nodeIds"][0].as_str().unwrap()).unwrap();
    assert_eq!((card.tag.as_str(), card.name.as_str()), ("div", "my-card"));
    let svg = doc.node(result["nodeIds"][1].as_str().unwrap()).unwrap();
    assert_eq!(tags(svg), ["rect"]);
    let html = html::document_html(doc, false);
    assert!(!html.contains("javascript") && html.contains("color:red"));
}
#[test]
fn lenient_import_keeps_rejecting_active_and_truncated_html() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let before = s.file(&fid(&s)).unwrap().clone();
    for fragment in [
        "<img src=x onerror=alert(1)>",
        "<a href=\"&#106;avascript:alert(1)\">x</a>",
        "<a href=\" java\tscript:alert(1)\">x</a>",
        "<object data=\"x.swf\"></object>",
        "<p>ok</p><script>alert(1)</script>",
        "<svg><a href=\"javascript:alert(1)\"><rect/></a></svg>",
        "<section><p>Unclosed section",
        "<div class=\"cut off",
        "<div><!-- unterminated",
    ] {
        assert!(
            write(
                &mut s,
                json!({"html":fragment,"targetNodeId":frame,"mode":"insert-children"})
            )
            .is_err(),
            "{fragment} was accepted"
        );
    }
    // <animate> could rewrite an href to javascript:; unknown SVG is dropped, never mapped.
    let result = edit(
        &mut s,
        "write_html",
        json!({"html":"<svg><a><animate attributeName=\"href\" to=\"javascript:alert(1)\"/></a></svg>","targetNodeId":frame,"mode":"insert-children"}),
    );
    assert!(
        result["warnings"][0]
            .as_str()
            .unwrap()
            .contains("<animate>")
    );
    assert!(!html::document_html(s.file(&fid(&s)).unwrap(), false).contains("javascript"));
    edit(&mut s, "undo", json!({}));
    assert_eq!(s.file(&fid(&s)).unwrap().pages, before.pages);
}
#[test]
fn write_html_imports_large_files_from_a_path() {
    let dir = temp_dir("import-path");
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let row = "<div style=\"padding:4px\">Row &amp; item</div>";
    let big = format!("<section>{}</section>", row.repeat(40_000));
    assert!(big.len() > 1024 * 1024);
    assert!(
        write(
            &mut s,
            json!({"html":big,"targetNodeId":frame,"mode":"insert-children"})
        )
        .unwrap_err()
        .contains("path")
    );
    let file = dir.join("page.html");
    std::fs::write(&file, &big).unwrap();
    let result = edit(
        &mut s,
        "write_html",
        json!({"path":file,"targetNodeId":frame,"mode":"insert-children"}),
    );
    let section = s
        .file(&fid(&s))
        .unwrap()
        .node(result["nodeIds"][0].as_str().unwrap())
        .unwrap();
    assert_eq!(section.children.len(), 40_000);
    assert_eq!(section.children[0].text, "Row & item");
    std::fs::write(dir.join("notes.txt"), "<p>x</p>").unwrap();
    for (args, message) in [
        (json!({"path":"page.html"}), "absolute"),
        (json!({"path":dir.join("notes.txt")}), ".html"),
        (json!({"path":dir.join("missing.html")}), "Cannot read"),
        (json!({"path":file,"html":"<p>x</p>"}), "not both"),
        (json!({}), "html or path"),
    ] {
        let mut args = args;
        args["targetNodeId"] = json!(frame);
        args["mode"] = json!("insert-children");
        let error = write(&mut s, args).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
}
#[test]
fn insert_index_counts_text_so_zero_lands_before_it() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let button = insert(&mut s, &frame, "<button>Save</button>")[0].clone();
    let icon = edit(
        &mut s,
        "write_html",
        json!({"html":"<svg viewBox=\"0 0 1 1\"><rect/></svg>","targetNodeId":button,"mode":"insert-children","index":0}),
    )["nodeIds"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let doc = s.file(&fid(&s)).unwrap();
    let node = doc.node(&button).unwrap();
    assert_eq!(node.text, "");
    assert_eq!(tags(node), ["svg", "span"]);
    assert_eq!(node.children[0].id, icon);
    assert_eq!(
        (
            node.children[1].name.as_str(),
            node.children[1].text.as_str()
        ),
        ("Text", "Save")
    );
    assert!(
        html::document_html(doc, false).contains("<rect></rect></svg><span>Save</span></button>")
    );
    // The same holds for moving an existing layer into a text leaf.
    let label = insert(&mut s, &frame, "<p>Label</p>")[0].clone();
    let dot = insert(&mut s, &frame, "<i>dot</i>")[0].clone();
    edit(
        &mut s,
        "move_nodes",
        json!({"moves":[{"nodeId":dot,"parentId":label,"index":0}]}),
    );
    let label = s.file(&fid(&s)).unwrap().node(&label).unwrap().clone();
    assert_eq!(tags(&label), ["i", "span"]);
    assert_eq!(label.children[1].text, "Label");
}
#[test]
fn misnested_html_is_repaired_the_way_browsers_do() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    insert(
        &mut s,
        &frame,
        "<b>1<p>2</b>3</p><table><tr><td>cell</td></tr>stray</table>",
    );
    let html = html::document_html(s.file(&fid(&s)).unwrap(), false);
    assert!(
        html.contains("<b>1</b><p><b>2</b><span>3</span></p><span>stray</span><table><tbody><tr><td>cell</td></tr></tbody></table>"),
        "{html}"
    );
}
#[test]
fn whitespace_between_inline_elements_survives_import_and_export() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let ids = insert(
        &mut s,
        &frame,
        concat!(
            "<p><span>Kenji Watanabe</span> <span>10:14</span>\n  <b>new</b></p>",
            "<div>\n  <p>a</p>\n  <p>b</p>\n  <span>c</span>\n</div>",
            "<div style=\"display:flex\"><span>a</span> <span>b</span></div>",
            "<span> <em>x</em></span>",
        ),
    );
    let doc = s.file(&fid(&s)).unwrap();
    let node = |i: usize| doc.node(&ids[i]).unwrap();
    let texts = |n: &cardboard::document::Node| {
        n.children
            .iter()
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(node(0)), ["Kenji Watanabe", " ", "10:14", " ", "new"]);
    // Indentation next to blocks, at block edges and in flex containers renders nothing.
    assert_eq!(tags(node(1)), ["p", "p", "span"]);
    assert_eq!(tags(node(2)), ["span", "span"]);
    // Inside an inline parent the leading space is not at a line edge, so it stays.
    assert_eq!(texts(node(3)), [" ", "x"]);
    let exported = html::node_html(node(0), false);
    assert!(
        exported.contains("Watanabe</span><span> </span><span>10:14</span>"),
        "{exported}"
    );
    let inline = html::node_html(node(3), false);
    let again = insert(&mut s, &frame, &format!("{exported}{inline}"));
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(
        html::node_html(doc.node(&again[0]).unwrap(), false),
        exported
    );
    assert_eq!(html::node_html(doc.node(&again[1]).unwrap(), false), inline);
}
#[test]
fn write_html_returns_ids_of_named_and_text_layers() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let mut html = String::from(
        r#"<div layer-name="Card"><h2>Pricing plans for every team size you could ever imagine</h2><button layer-name="Buy">Buy</button><span layer-name="Price">$9</span><span layer-name="Price">$19</span>"#,
    );
    for i in 0..15 {
        html.push_str(&format!("<p>Row {i}</p>"));
    }
    html.push_str("</div>");
    let result = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":html}),
    );
    let doc = s.file(&fid(&s)).unwrap();
    let layers = &result["layers"];
    assert_eq!(layers["Card"], result["nodeIds"][0]);
    assert_eq!(
        doc.node(layers["Buy"].as_str().unwrap()).unwrap().text,
        "Buy"
    );
    let prices = layers["Price"].as_array().unwrap();
    assert_eq!(prices.len(), 2);
    assert_eq!(doc.node(prices[1].as_str().unwrap()).unwrap().text, "$19");
    let texts = result["texts"].as_array().unwrap();
    assert_eq!(texts.len(), 12);
    assert_eq!(texts[0]["tag"], "h2");
    assert!(texts[0]["text"].as_str().unwrap().ends_with('…'));
    assert_eq!(texts[1]["text"], "Row 0");
    assert_eq!(result["moreTexts"], 4);
    // Plain writes stay as small as before.
    let plain = edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":frame,"mode":"insert-children","html":"<div></div>"}),
    );
    assert!(
        plain.get("layers").is_none() && plain.get("texts").is_none(),
        "{plain}"
    );
}

fn card_page(s: &mut Store) -> (String, Vec<String>) {
    let frame = artboard(s);
    let ids = insert(
        s,
        &frame,
        r#"<div layer-name="Card" style="background-color:#000;padding:8px"><button layer-name="Buy button" style="color:var(--color-accent);background-color:#000">Buy now</button><p style="color:#555">Free shipping</p></div>"#,
    );
    (frame, ids)
}
fn found(s: &mut Store, args: Value) -> Vec<String> {
    edit(s, "find_nodes", args)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn find_nodes_filters_by_text_name_tag_and_styles_within_a_scope() {
    let mut s = Store::memory();
    let (frame, ids) = card_page(&mut s);
    let card = &ids[0];
    let children = s
        .file(&fid(&s))
        .unwrap()
        .node(card)
        .unwrap()
        .children
        .clone();
    let (button, para) = (&children[0].id, &children[1].id);
    assert_eq!(found(&mut s, json!({"text":"buy NOW"})), [button.as_str()]);
    assert_eq!(found(&mut s, json!({"name":"button"})), [button.as_str()]);
    assert_eq!(found(&mut s, json!({"tag":"P"})), [para.as_str()]);
    assert_eq!(
        found(&mut s, json!({"styles":{"backgroundColor":"#000"}})),
        [card.clone(), button.clone()]
    );
    assert_eq!(
        found(
            &mut s,
            json!({"styles":{"*":{"contains":"var(--color-accent)"}}})
        ),
        [button.as_str()]
    );
    assert_eq!(
        found(&mut s, json!({"styles":{"padding":true,"color":false}})),
        [card.as_str()]
    );
    // All filters must match, and a nodeId scope includes the node itself.
    assert!(
        found(
            &mut s,
            json!({"tag":"p","styles":{"background-color":"#000"}})
        )
        .is_empty()
    );
    assert_eq!(
        found(&mut s, json!({"nodeId":button,"styles":{"*":true}})),
        [button.as_str()]
    );
    assert_eq!(
        found(&mut s, json!({"pageId":"page-1","tag":"div"})),
        [frame, card.clone()]
    );
    let limited = edit(&mut s, "find_nodes", json!({"styles":{"*":true},"limit":1}));
    assert_eq!(limited["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(
        (limited["total"].as_u64(), limited["truncated"].as_bool()),
        (Some(4), Some(true))
    );
    assert_eq!(limited["nodes"][0]["styles"]["width"], "1440px");
    assert!(tools::call(&mut s, "find_nodes", &json!({"pageId":"page-99"})).is_err());
}

#[test]
fn bulk_edits_by_query_are_one_undo_step_and_report_counts() {
    let mut s = Store::memory();
    let (_, ids) = card_page(&mut s);
    let revision = s.file(&fid(&s)).unwrap().revision;
    let result = edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"query":{"styles":{"background-color":"#000"}},"styles":{"backgroundColor":"#111"}}]}),
    );
    assert_eq!(
        (result["matched"].as_u64(), result["changed"].as_u64()),
        (Some(2), Some(2))
    );
    assert_eq!(result["revision"].as_u64(), Some(revision + 1));
    assert_eq!(
        found(&mut s, json!({"styles":{"background-color":"#111"}})).len(),
        2
    );
    let again = edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"query":{"tag":"div","nodeId":ids[0]},"styles":{"background-color":"#111"}}]}),
    );
    assert_eq!(
        (again["matched"].as_u64(), again["changed"].as_u64()),
        (Some(1), Some(0))
    );
    edit(&mut s, "undo", json!({}));
    assert_eq!(
        found(&mut s, json!({"styles":{"background-color":"#000"}})).len(),
        2
    );

    let attrs = edit(
        &mut s,
        "set_attributes",
        json!({"updates":[{"query":{"tag":"button"},"attributes":{"type":"button"}}]}),
    );
    assert_eq!(attrs["changed"].as_u64(), Some(1));
    let deleted = edit(&mut s, "delete_nodes", json!({"query":{"text":"shipping"}}));
    assert_eq!(deleted["count"].as_u64(), Some(1));
    assert!(found(&mut s, json!({"tag":"p"})).is_empty());
    // A query without filters would match the whole file.
    assert!(
        tools::call(
            &mut s,
            "delete_nodes",
            &json!({"query":{"pageId":"page-1"}})
        )
        .is_err()
    );
    assert!(tools::call(&mut s, "delete_nodes", &json!({})).is_err());
}

#[test]
fn bare_numeric_ids_are_accepted() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let number = frame.trim_start_matches("node-").to_owned();
    assert_eq!(
        edit(&mut s, "get_node_info", json!({"nodeId":number}))["id"],
        frame
    );
    let n: u64 = number.parse().unwrap();
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[n],"styles":{"color":"red"}}]}),
    );
    edit(
        &mut s,
        "write_html",
        json!({"targetNodeId":number,"mode":"insert-children","html":"<p>Hi</p>"}),
    );
    assert_eq!(found(&mut s, json!({"nodeId":number,"tag":"p"})).len(), 1);
    edit(&mut s, "set_active_page", json!({"pageId":"1"}));
    // Attribute and style payloads are not ids.
    edit(
        &mut s,
        "set_attributes",
        json!({"updates":[{"nodeId":number,"attributes":{"id":"42"}}]}),
    );
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.node(&frame).unwrap().attributes["id"], "42");
    assert_eq!(doc.node(&frame).unwrap().styles["color"], "red");
}

#[test]
fn batch_runs_reads_against_its_own_edits() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let result = edit(
        &mut s,
        "batch",
        json!({"operations":[
            {"tool":"update_styles","args":{"updates":[{"nodeIds":[frame],"styles":{"color":"red"}}]}},
            {"tool":"get_styles","args":{"nodeIds":[frame]}},
            {"tool":"find_nodes","args":{"styles":{"color":"red"}}}
        ]}),
    );
    assert_eq!(result["results"][1]["styles"][&frame]["color"], "red");
    assert_eq!(result["results"][2]["total"].as_u64(), Some(1));
    let err = tools::call(&mut s, "batch", &json!({"operations":[{"tool":"undo"}]})).unwrap_err();
    assert!(err.contains("undo cannot run inside batch") && err.contains("find_nodes"));
}

#[test]
fn stale_revision_error_names_the_current_revision() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let err = tools::call(
        &mut s,
        "update_styles",
        &json!({"expectedRevision":0,"updates":[{"nodeIds":[frame],"styles":{"color":"red"}}]}),
    )
    .unwrap_err();
    assert!(
        err.contains("revision 1") && err.contains("retry with expectedRevision 1"),
        "{err}"
    );
}

#[test]
fn reads_are_scoped_and_bounded() {
    let mut s = Store::memory();
    let (frame, ids) = card_page(&mut s);
    let page = edit(&mut s, "create_page", json!({"name":"Other"}))["pageId"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = edit(
        &mut s,
        "create_artboard",
        json!({"name":"Phone","pageId":page,"styles":{"width":"390px","height":"844px"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_owned();

    let card = edit(&mut s, "export_html", json!({"nodeId":ids[0]}))["html"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(card.contains("Buy now") && !card.contains("width:1440px"));
    let paged = edit(&mut s, "export_html", json!({"pageId":page}))["html"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(paged.contains("width:390px") && !paged.contains("Buy now"));
    let clipped = edit(&mut s, "export_html", json!({"maxChars":300}));
    assert_eq!(clipped["truncated"], true);
    assert!(clipped["html"].as_str().unwrap().contains("[truncated: "));
    assert!(clipped["totalChars"].as_u64().unwrap() > 300);
    assert!(
        edit(&mut s, "export_html", json!({"maxChars":0}))
            .get("truncated")
            .is_none()
    );

    let node = edit(&mut s, "get_document", json!({"nodeId":frame,"depth":0}))["node"].clone();
    assert_eq!(node["children"], json!([]));
    assert_eq!(node["omittedChildren"], 1);
    let p = edit(&mut s, "get_document", json!({"pageId":page}))["page"].clone();
    assert_eq!(p["artboards"][0]["id"], other);
    let whole = edit(&mut s, "get_document", json!({"depth":1}));
    assert_eq!(
        whole["document"]["pages"][0]["artboards"][0]["children"][0]["omittedChildren"],
        2
    );
    let err = tools::call(&mut s, "get_document", &json!({"maxChars":100})).unwrap_err();
    assert!(err.contains("Narrow it with pageId"), "{err}");

    let tree = edit(&mut s, "get_tree_summary", json!({"pageId":page}))["tree"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(tree.contains(&other) && !tree.contains(&frame));
    assert!(
        edit(&mut s, "get_tree_summary", json!({}))["tree"]
            .as_str()
            .unwrap()
            .contains(&frame)
    );
}

#[test]
fn list_files_omits_previews_unless_asked() {
    let dir = temp_dir("list-previews");
    let mut s = Store::library(&dir, None).unwrap();
    edit(&mut s, "create_file", json!({"name":"Shop"}));
    card_page(&mut s);
    s.flush().unwrap();
    let plain = edit(&mut s, "list_files", json!({}))["files"][0]["artboards"][0].clone();
    assert!(plain.get("html").is_none() && plain["name"] == "Desktop");
    let rich =
        edit(&mut s, "list_files", json!({"previews":true}))["files"][0]["artboards"][0].clone();
    assert!(rich["html"].as_str().unwrap().contains("Buy now"));
}

/// Rendering tests need a Chromium-based browser; skip (loudly) where none is installed.
fn renderer() -> bool {
    let found = cardboard::render::find_browser().is_ok();
    if !found {
        eprintln!("skipping: no Chromium-based browser for rendering");
    }
    found
}
fn png_size(data: &str) -> (u32, u32) {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
    (be(16), be(20))
}

#[test]
fn screenshot_renders_unsaved_edits_and_crops_nodes() {
    if !renderer() {
        return;
    }
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[frame],"styles":{"width":"600px","height":"fit-content","padding":"40px"}}]}),
    );
    let card = insert(
        &mut s,
        &frame,
        "<div style=\"width:200px;height:120px;background:var(--color-accent)\"></div>",
    )[0]
    .clone();
    edit(
        &mut s,
        "create_tokens",
        json!({"tokens":[{"name":"--color-accent","value":"#2f7d4f"}]}),
    );
    let shot = edit(&mut s, "screenshot", json!({"nodeId":frame}));
    assert_eq!(
        shot["bounds"],
        json!({"x":0.0,"y":0.0,"width":600.0,"height":200.0})
    );
    assert_eq!(
        png_size(shot["image"]["data"].as_str().unwrap()),
        (600, 200)
    );
    // Nodes are cropped from the artboard's layout and default to 2x.
    let shot = edit(&mut s, "screenshot", json!({"nodeId":card}));
    assert_eq!(
        shot["bounds"],
        json!({"x":40.0,"y":40.0,"width":200.0,"height":120.0})
    );
    assert_eq!(shot["scale"], 2.0);
    assert_eq!(
        png_size(shot["image"]["data"].as_str().unwrap()),
        (400, 240)
    );
    // Huge artboards are scaled down to keep the payload reasonable.
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[frame],"styles":{"width":"1440px","height":"6000px"}}]}),
    );
    let shot = edit(&mut s, "screenshot", json!({"nodeId":frame}));
    assert!(shot["scale"].as_f64().unwrap() < 1.0 && shot["note"].is_string());
    let (w, h) = png_size(shot["image"]["data"].as_str().unwrap());
    assert!(w * h <= 4_000_000 + 10_000, "{w}x{h}");
    assert!(
        tools::call(
            &mut s,
            "screenshot",
            &json!({"nodeId":frame,"path":"x.png"})
        )
        .is_err()
    );
    assert!(
        tools::call(
            &mut s,
            "batch",
            &json!({"operations":[{"tool":"screenshot","args":{"nodeId":frame}}]})
        )
        .is_err()
    );
}

#[test]
fn get_bounds_measures_layout_overflow_and_computed_styles() {
    if !renderer() {
        return;
    }
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    edit(
        &mut s,
        "update_styles",
        json!({"updates":[{"nodeIds":[frame],"styles":{"width":"400px","height":"300px","padding":"20px","flexDirection":"column","gap":"10px"}}]}),
    );
    let ids = insert(
        &mut s,
        &frame,
        "<p style=\"margin:0;width:80px;white-space:nowrap;font-size:20px\">Far too long for this box</p><div style=\"width:500px;height:20px\"></div>",
    );
    let bounds = edit(
        &mut s,
        "get_bounds",
        json!({"nodeIds":[frame],"depth":1,"styles":["fontSize"]}),
    );
    let nodes = bounds["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    let by_id = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();
    assert_eq!(by_id(&frame)["width"], 400.0);
    let text = by_id(&ids[0]);
    assert_eq!(
        (text["x"].as_f64(), text["y"].as_f64()),
        (Some(20.0), Some(20.0))
    );
    assert!(text["overflowX"].as_f64().unwrap() > 50.0);
    assert_eq!(text["styles"]["font-size"], "20px");
    assert_eq!(text["parentId"], json!(frame));
    let wide = by_id(&ids[1]);
    assert_eq!(wide["outsideParent"], true);
    assert_eq!(
        wide["y"].as_f64().unwrap(),
        20.0 + text["height"].as_f64().unwrap() + 10.0
    );
    edit(
        &mut s,
        "set_node_state",
        json!({"updates":[{"nodeId":ids[1],"visible":false}]}),
    );
    let bounds = edit(&mut s, "get_bounds", json!({"nodeIds":[ids[1]],"depth":0}));
    assert_eq!(bounds["nodes"][0]["hidden"], true);
    let err = tools::call(&mut s, "screenshot", &json!({"nodeId":ids[1]})).unwrap_err();
    assert!(err.contains("no visible size"), "{err}");
}

#[test]
fn get_bounds_ignores_glyph_overhang_and_separates_deliberate_clipping() {
    if !renderer() {
        return;
    }
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let ids = insert(
        &mut s,
        &frame,
        concat!(
            r#"<div style="width:600px"><h1 style="margin:0;font:italic 96px/0.8 serif">Jagged type</h1></div>"#,
            r#"<div style="width:100px;height:40px;overflow:hidden"><div style="width:200px;height:80px;flex:none"></div></div>"#,
            r#"<div style="width:120px;height:30px;overflow:hidden"><p style="margin:0;font-size:16px">Long text that wraps over several lines in a small box</p></div>"#,
            r#"<div style="height:20px;flex:none"><div style="height:60px"></div></div>"#,
        ),
    );
    let bounds = edit(&mut s, "get_bounds", json!({"nodeIds":[frame],"depth":3}));
    let nodes = bounds["nodes"].as_array().unwrap();
    let by_id = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();
    let child = |id: &str| {
        let doc = s.file(&fid(&s)).unwrap();
        doc.node(id).unwrap().children[0].id.clone()
    };
    let keys = |n: &Value| {
        let mut k: Vec<_> = n.as_object().unwrap().keys().cloned().collect();
        k.retain(|k| k.starts_with("overflow") || k.starts_with("clipped") || k == "outsideParent");
        k
    };
    // A tight line-height lets descenders poke out of the line box; that is not overflow.
    assert!(keys(by_id(&frame)).is_empty(), "{}", by_id(&frame));
    assert!(keys(by_id(&ids[0])).is_empty(), "{}", by_id(&ids[0]));
    assert!(keys(by_id(&child(&ids[0]))).is_empty());
    // Deliberate clipping is reported softly, and the clipped child names its clipper.
    let mask = by_id(&ids[1]);
    assert_eq!(
        (mask["clippedX"].as_f64(), mask["clippedY"].as_f64()),
        (Some(100.0), Some(40.0))
    );
    assert_eq!(keys(mask), ["clippedX", "clippedY"]);
    let photo = by_id(&child(&ids[1]));
    assert_eq!(photo["clippedBy"], json!(ids[1]));
    assert!(photo.get("outsideParent").is_none());
    // Text cut off by a clipping box is still reported.
    assert!(by_id(&ids[2])["clippedY"].as_f64().unwrap() > 20.0);
    assert_eq!(by_id(&child(&ids[2]))["clippedBy"], json!(ids[2]));
    // Visible spill stays overflow.
    assert_eq!(by_id(&ids[3])["overflowY"], 40.0);
    assert_eq!(by_id(&child(&ids[3]))["outsideParent"], true);
}

#[test]
fn render_tools_accept_bare_ids_outside_tools_call() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let ids = insert(&mut s, &frame, "<div>Hi</div>");
    let bare = ids[0].trim_start_matches("node-").parse::<u64>().unwrap();
    // The MCP hub prepares renders directly, so normalization must happen there too.
    cardboard::render::prepare(&s, "screenshot", &json!({"nodeId":bare})).unwrap();
    cardboard::render::prepare(&s, "get_bounds", &json!({"nodeIds":[bare.to_string()]})).unwrap();
}

#[test]
fn token_modes_render_per_artboard_and_follow_renames() {
    let mut s = Store::memory();
    edit(
        &mut s,
        "create_tokens",
        json!({"tokens":[{"name":"--bg","values":{"light":"#ffffff","dark":"#0a0a0a"}},{"name":"--surface","value":"var(--bg)"}]}),
    );
    let dark = edit(
        &mut s,
        "create_artboard",
        json!({"name":"Dark","mode":"dark","styles":{"width":"400px","height":"300px","background":"var(--surface)"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.node(&dark).unwrap().attributes["data-mode"], "dark");
    let out = html::document_html(doc, false);
    // Each mode re-declares aliases so var(--bg) resolves inside the dark artboard.
    assert!(out.contains(":root{--bg:#ffffff;--surface:var(--bg)}"));
    assert!(out.contains("[data-mode=\"dark\"]{--bg:#0a0a0a;--surface:var(--bg)}"));
    assert!(out.contains("data-mode=\"dark\""));

    let tokens = edit(&mut s, "get_tokens", json!({"mode":"dark"}));
    assert_eq!(tokens["modes"], json!(["light", "dark"]));
    assert_eq!(tokens["tokens"][0]["value"], "#0a0a0a");
    assert_eq!(tokens["tokens"][0]["values"]["light"], "#ffffff");

    edit(
        &mut s,
        "set_tokens",
        json!({"tokens":[{"name":"--bg","newName":"--ground"}],"defaultMode":"dark"}),
    );
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.modes["dark"]["--ground"], "#0a0a0a");
    assert_eq!(doc.tokens["--surface"], "var(--ground)");
    assert!(html::document_html(doc, false).contains(":root{--ground:#0a0a0a"));

    // Deleting the only dark override removes the mode and resets the default.
    edit(
        &mut s,
        "set_tokens",
        json!({"mode":"dark","tokens":[{"name":"--ground","delete":true}]}),
    );
    let doc = s.file(&fid(&s)).unwrap();
    assert!(doc.modes.is_empty() && doc.default_mode.is_none());
    assert_eq!(doc.tokens["--ground"], "#ffffff");
    assert!(tools::call(&mut s, "set_tokens", &json!({"defaultMode":"dark"})).is_err());
    assert!(
        tools::call(
            &mut s,
            "create_tokens",
            &json!({"tokens":[{"name":"--x","value":"1px","mode":"Dark Mode"}]})
        )
        .is_err()
    );
}

#[test]
fn apply_theme_imports_tweakcn_css_and_exports_shadcn() {
    let mut s = Store::memory();
    let css = r#"
/* pasted from tweakcn */
:root {
  --background: oklch(1 0 0);
  --foreground: oklch(0.145 0 0);
  --radius: 0.625rem;
  --font-sans: "Inter", sans-serif;
  --shadow-sm: 0 1px 2px rgb(0 0 0 / 0.05);
}
.dark {
  --background: oklch(0.145 0 0);
  --foreground: oklch(0.985 0 0);
  --radius: 0.625rem;
}
@theme inline {
  --color-background: var(--background);
}
@layer base { body { color: red; } }
"#;
    edit(&mut s, "apply_theme", json!({"css":css}));
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.tokens["--background"], "oklch(1 0 0)");
    assert_eq!(doc.tokens["--font-sans"], "\"Inter\", sans-serif");
    // Only real differences are stored per mode: the pasted dark values, plus stock shadcn dark
    // values for the tokens the paste left out.
    assert_eq!(doc.modes["dark"]["--background"], "oklch(0.145 0 0)");
    assert!(!doc.modes["dark"].contains_key("--radius"));
    assert_eq!(doc.tokens["--chart-1"], "oklch(0.646 0.222 41.116)");
    assert!(!doc.tokens.contains_key("--color-background"));

    let out = edit(&mut s, "get_tokens", json!({"format":"shadcn"}))["css"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(out.starts_with(":root {\n  --radius: 0.625rem;\n  --background: oklch(1 0 0);"));
    assert!(out.contains(".dark {\n  --radius: 0.625rem;\n  --background: oklch(0.145 0 0);"));
    assert!(out.contains("@theme inline {"));
    assert!(out.contains("  --color-background: var(--background);"));
    assert!(out.contains("  --radius-sm: calc(var(--radius) * 0.6);"));
    assert!(out.contains("  --font-sans: var(--font-sans);"));
    assert!(!out.contains("--color-radius"));

    // Round trip: the exported CSS imports back to the same tokens.
    let before = s.file(&fid(&s)).unwrap().clone();
    edit(&mut s, "apply_theme", json!({"css":out,"replace":true}));
    let after = s.file(&fid(&s)).unwrap();
    assert_eq!(before.tokens, after.tokens);
    assert_eq!(before.modes, after.modes);

    assert!(tools::call(&mut s, "apply_theme", &json!({"css":"body { color: red }"})).is_err());
    assert!(
        tools::call(
            &mut s,
            "apply_theme",
            &json!({"css":":root { --x: </style>; }"})
        )
        .is_err()
    );
}

#[test]
fn shadcn_is_the_only_preset_and_pasted_themes_get_extension_defaults() {
    assert_eq!(cardboard::theme::PRESETS, ["shadcn"]);
    let mut s = Store::memory();
    edit(&mut s, "apply_theme", json!({"preset":"shadcn"}));
    let doc = s.file(&fid(&s)).unwrap();
    doc.validate().unwrap();
    for name in [
        "--background",
        "--primary",
        "--card-ring",
        "--field-bg",
        "--radius-control",
    ] {
        assert!(doc.tokens.contains_key(name), "shadcn lacks {name}");
    }
    assert!(!doc.modes["dark"].is_empty(), "shadcn has no dark values");
    let error = tools::call(&mut s, "apply_theme", &json!({"preset":"material"})).unwrap_err();
    assert!(error.contains("css"), "{error}");

    // A small pasted theme keeps its own values and gets neutral extension tokens.
    let mut s = Store::memory();
    let css = ":root { --background: #fafafa; --foreground: #111; --primary: #2563eb; --radius: 0.5rem; --radius-chip: 2px; }\n.dark { --background: #0b0b0c; --foreground: #eee; }";
    edit(&mut s, "apply_theme", json!({"css":css}));
    let doc = s.file(&fid(&s)).unwrap();
    doc.validate().unwrap();
    assert_eq!(doc.tokens["--primary"], "#2563eb");
    assert_eq!(doc.tokens["--radius-chip"], "2px");
    assert_eq!(doc.tokens["--signal"], "var(--primary)");
    assert_eq!(doc.tokens["--field-border"], "var(--input)");
    assert_eq!(doc.modes["dark"]["--background"], "#0b0b0c");
}

#[test]
fn create_kit_builds_token_only_artboards_in_one_undo_step() {
    let mut s = Store::memory();
    let kit = edit(&mut s, "create_kit", json!({}));
    assert_eq!(kit["modes"], json!(["light", "dark"]));
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.pages.len(), 2);
    let page = &doc.pages[1];
    assert_eq!(page.name, "Design system");
    assert_eq!(page.artboards.len(), 2);
    assert_eq!(page.artboards[1].attributes["data-mode"], "dark");
    assert!(page.artboards[1].position.unwrap()[0] > page.artboards[0].position.unwrap()[0]);
    // Every color in the kit comes from a token, so editing tokens restyles all of it.
    fn literal_colors(n: &cardboard::document::Node, found: &mut Vec<String>) {
        for (k, v) in &n.styles {
            if v.contains('#') || v.contains("rgb(") || v.contains("rgba(") || v.contains("oklch(")
            {
                found.push(format!("{} {k}: {v}", n.name));
            }
        }
        for c in &n.children {
            literal_colors(c, found);
        }
    }
    let mut found = vec![];
    for ab in &page.artboards {
        literal_colors(ab, &mut found);
    }
    assert!(found.is_empty(), "{found:?}");
    assert!(page.artboards[0].count() > 300);
    edit(&mut s, "undo", json!({}));
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.pages.len(), 1);
    assert!(doc.tokens.is_empty());
}

#[test]
fn kit_screenshots_carry_every_mode_and_its_icons_are_icon_layers() {
    let mut s = Store::memory();
    let kit = edit(&mut s, "create_kit", json!({}));
    let dark = kit["nodeIds"][1].as_str().unwrap().to_string();
    // The render page declares the dark set, which the dark artboard opts into.
    let job = cardboard::render::prepare(&s, "screenshot", &json!({"nodeId":dark})).unwrap();
    let html = job.html();
    assert!(html.contains("[data-mode=\"dark\"]{"), "{html}");
    assert!(html.contains("data-mode=\"dark\""));
    assert!(html.contains("<path"), "icon markup must render");
    let icons = edit(
        &mut s,
        "find_nodes",
        json!({"nodeId":dark,"tag":"icon","limit":500}),
    );
    assert!(icons["total"].as_u64().unwrap() > 5, "{icons}");
    assert!(
        icons["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["icon"].as_str().unwrap().starts_with("lucide:"))
    );
}

#[test]
fn partial_themes_are_completed_from_stock_shadcn_per_mode() {
    let preset = cardboard::theme::preset("shadcn").unwrap();
    // Pasted CSS with a dark block: every contract token exists in both modes.
    let mut s = Store::memory();
    let css = ":root { --primary: #2563eb; --radius: 0.5rem; }\n.dark { --primary: #60a5fa; }";
    edit(&mut s, "apply_theme", json!({"css":css}));
    let doc = s.file(&fid(&s)).unwrap();
    doc.validate().unwrap();
    for name in preset["light"].keys() {
        assert!(doc.tokens.contains_key(name), "missing {name}");
    }
    assert_eq!(doc.tokens["--primary"], "#2563eb");
    assert_eq!(doc.modes["dark"]["--primary"], "#60a5fa");
    assert_eq!(
        doc.tokens["--destructive"],
        preset["light"]["--destructive"]
    );
    assert_eq!(
        doc.modes["dark"]["--destructive"],
        preset["dark"]["--destructive"]
    );
    assert_eq!(doc.modes["dark"]["--chart-1"], preset["dark"]["--chart-1"]);
    let dark = cardboard::theme::resolved(doc, "dark");
    for name in preset["dark"].keys() {
        assert!(dark.contains_key(name), "dark lacks {name}");
    }

    // Direct values, light only: filled too, and no dark mode appears out of nowhere.
    let mut s = Store::memory();
    edit(
        &mut s,
        "apply_theme",
        json!({"modes":{"light":{"--background":"#fff","--foreground":"#111"}}}),
    );
    let doc = s.file(&fid(&s)).unwrap();
    assert_eq!(doc.tokens["--background"], "#fff");
    assert_eq!(doc.tokens["--chart-3"], preset["light"]["--chart-3"]);
    assert!(doc.modes.is_empty());
}

#[test]
fn instance_layer_state_set_alongside_the_main_sticks() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let main = insert(&mut s, &frame, "<div><span>Old</span></div>")[0].clone();
    let old = s.file(&fid(&s)).unwrap().node(&main).unwrap().children[0]
        .id
        .clone();
    edit(&mut s, "create_component", json!({"nodeId":main}));
    let [a, b] = [0, 1].map(|_| {
        edit(
            &mut s,
            "insert_instance",
            json!({"componentId":main,"parentId":frame}),
        )["nodeId"]
            .as_str()
            .unwrap()
            .to_string()
    });
    let added = insert(&mut s, &main, "<p>New</p>")[0].clone();
    let visible = |s: &mut Store, id: &str| {
        let info = edit(s, "get_node_info", json!({"nodeId":id}));
        assert_eq!(
            info["visible"],
            s.file(&fid(s)).unwrap().node(id).unwrap().visible
        );
        info["visible"].as_bool().unwrap()
    };
    for layer in [&added, &old] {
        let (on_a, on_b) = (format!("{a}:{layer}"), format!("{b}:{layer}"));
        // Hide the layer on its own, then hide it in the main but keep it on `a` in one call.
        edit(
            &mut s,
            "set_node_state",
            json!({"updates":[{"nodeId":on_b,"visible":false}]}),
        );
        assert!(!visible(&mut s, &on_b));
        edit(
            &mut s,
            "set_node_state",
            json!({"updates":[{"nodeId":layer,"visible":false},{"nodeId":on_a,"visible":true}]}),
        );
        assert!(!visible(&mut s, layer) && visible(&mut s, &on_a) && !visible(&mut s, &on_b));
        // Show it in the main again but keep `b` hiding it, in one call.
        edit(
            &mut s,
            "set_node_state",
            json!({"updates":[{"nodeId":layer,"visible":true},{"nodeId":on_b,"visible":false}]}),
        );
        assert!(visible(&mut s, layer) && visible(&mut s, &on_a) && !visible(&mut s, &on_b));
        // Another main edit rebuilds the instances without losing either override.
        edit(
            &mut s,
            "update_styles",
            json!({"updates":[{"nodeIds":[main],"styles":{"padding":"4px"}}]}),
        );
        assert!(visible(&mut s, &on_a) && !visible(&mut s, &on_b));
    }
    // Listing the main and a fresh instance with the same value is still a main edit.
    let c = edit(
        &mut s,
        "insert_instance",
        json!({"componentId":main,"parentId":frame}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    let on_c = format!("{c}:{added}");
    edit(
        &mut s,
        "set_node_state",
        json!({"updates":[{"nodeId":added,"visible":false},{"nodeId":on_c,"visible":false}]}),
    );
    edit(
        &mut s,
        "set_node_state",
        json!({"updates":[{"nodeId":added,"visible":true}]}),
    );
    assert!(visible(&mut s, &on_c));
}

#[test]
fn write_html_mode_defaults_to_insert_children() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let ids = edit(
        &mut s,
        "write_html",
        json!({"html":"<p>Hi</p>","targetNodeId":frame}),
    )["nodeIds"]
        .clone();
    let id = ids[0].as_str().unwrap();
    let doc = s.file(&fid(&s)).unwrap();
    assert!(
        doc.node(&frame)
            .unwrap()
            .children
            .iter()
            .any(|c| c.id == id)
    );
}

#[test]
fn landing_guide_is_its_own_topic() {
    let mut s = Store::memory();
    let main = tools::call(&mut s, "get_guide", &json!({"topic":"cardboard-design"})).unwrap();
    assert!(
        main["guide"]
            .as_str()
            .unwrap()
            .contains("cardboard-landing")
    );
    let landing = tools::call(&mut s, "get_guide", &json!({"topic":"cardboard-landing"})).unwrap();
    assert!(
        landing["guide"]
            .as_str()
            .unwrap()
            .contains("Refinement pass")
    );
}

#[test]
fn write_html_keeps_svg_filters_for_grain() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let html = r##"<svg width="200" height="200"><filter id="grain"><feTurbulence type="fractalNoise" baseFrequency="0.8" numOctaves="2" stitchTiles="stitch"/><feColorMatrix type="saturate" values="0"/></filter><rect width="100%" height="100%" filter="url(#grain)"/></svg>"##;
    let out = edit(
        &mut s,
        "write_html",
        json!({"html":html,"targetNodeId":frame}),
    );
    assert!(out.get("warnings").is_none(), "{out}");
    let doc = s.file(&fid(&s)).unwrap();
    let markup = cardboard::html::node_html(doc.node(&frame).unwrap(), false);
    assert!(markup.contains("<feTurbulence"), "{markup}");
    assert!(markup.contains(r#"baseFrequency="0.8""#), "{markup}");
    assert!(markup.contains(r#"filter="url(#grain)""#), "{markup}");
}

#[test]
fn write_html_ignores_indentation_inside_svg_groups() {
    let mut s = Store::memory();
    let frame = artboard(&mut s);
    let html = "<p>a <svg width=\"20\" height=\"20\">\n  <g>\n    <rect width=\"4\" height=\"4\"/>\n    <circle r=\"2\"/>\n  </g>\n  <text x=\"0\" y=\"10\">A <tspan>B</tspan></text>\n</svg> b</p>";
    edit(
        &mut s,
        "write_html",
        json!({"html":html,"targetNodeId":frame}),
    );
    let doc = s.file(&fid(&s)).unwrap();
    let markup = cardboard::html::node_html(doc.node(&frame).unwrap(), false);
    let g = &markup[markup.find("<g").unwrap()..markup.find("</g>").unwrap()];
    assert!(!g.contains("<span"), "{markup}");
    assert!(
        markup.contains("A <tspan") || markup.contains("A </span><tspan"),
        "{markup}"
    );
}
