use base64::{Engine, engine::general_purpose::STANDARD};
use cardboard::{assets, document::Store, stock, store, tools};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn edit(s: &mut Store, name: &str, args: Value) -> Value {
    tools::call(s, name, &args).unwrap()
}
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cardboard-assets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
/// The start of a PNG: signature and IHDR, enough for type and size detection.
fn png(w: u32, h: u32, salt: u8) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, salt]);
    b
}
fn library_with_file(name: &str) -> (PathBuf, Store, String, String) {
    let dir = temp_dir(name);
    let mut s = Store::library(&dir, None).unwrap();
    let file = edit(&mut s, "create_file", json!({"name":"Design"}))["fileId"]
        .as_str()
        .unwrap()
        .to_string();
    let frame = edit(
        &mut s,
        "create_artboard",
        json!({"fileId":file,"name":"Desktop","styles":{"width":"1440px","height":"900px"}}),
    )["nodeId"]
        .as_str()
        .unwrap()
        .to_string();
    (dir, s, file, frame)
}
fn stored(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir.join(assets::DIR))
        .map(|d| {
            d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn local_image_import_stores_once_and_inserts_a_sized_img() {
    let (dir, mut s, file, frame) = library_with_file("import");
    let source = dir.join("photo.png");
    std::fs::write(&source, png(1600, 900, 1)).unwrap();
    let plain = edit(
        &mut s,
        "import_image",
        json!({"path":source.to_string_lossy()}),
    );
    let src = plain["src"].as_str().unwrap();
    assert!(src.starts_with("/asset/") && src.ends_with(".png"), "{src}");
    assert_eq!(
        (plain["width"].as_u64(), plain["height"].as_u64()),
        (Some(1600), Some(900))
    );
    let placed = edit(
        &mut s,
        "import_image",
        json!({"fileId":file,"path":source.to_string_lossy(),"parentId":frame,"alt":"Harbor at dusk"}),
    );
    assert_eq!(placed["src"], plain["src"]);
    assert_eq!(stored(&dir).len(), 1, "content-addressed: one copy");
    let node = s
        .file(&file)
        .unwrap()
        .node(placed["nodeId"].as_str().unwrap())
        .unwrap()
        .clone();
    assert_eq!(node.tag, "img");
    assert_eq!(node.attributes["src"], src);
    assert_eq!(node.attributes["alt"], "Harbor at dusk");
    assert_eq!(node.styles["width"], "800px");
    assert_eq!(node.styles["height"], "450px");
    // Undo removes the layer in one step.
    s.undo(&file).unwrap();
    assert!(s.file(&file).unwrap().node(node.id.as_str()).is_err());

    // Content is checked, not the file name.
    let text = dir.join("notes.png");
    std::fs::write(&text, "not an image").unwrap();
    let error = tools::call(
        &mut s,
        "import_image",
        &json!({"path":text.to_string_lossy()}),
    )
    .unwrap_err();
    assert!(error.contains("Unsupported file"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn base64_images_move_to_the_store_and_files_stay_portable() {
    let (dir, mut s, file, frame) = library_with_file("portable");
    let data = STANDARD.encode(png(10, 20, 2));
    edit(
        &mut s,
        "write_html",
        json!({"fileId":file,"targetNodeId":frame,"mode":"insert-children",
            "html":format!("<div style=\"background-image:url(&quot;data:image/png;base64,{data}&quot;)\"><img src=\"data:image/png;base64,{data}\" /></div>")}),
    );
    let doc = s.file(&file).unwrap().clone();
    let json = serde_json::to_string(&doc).unwrap();
    assert!(!json.contains("base64"), "document keeps no base64");
    let names = stored(&dir);
    assert_eq!(names.len(), 1);
    assert!(json.contains(&format!("/asset/{}", names[0])));

    // Export inlines by default, or links into the store.
    let inline = edit(&mut s, "export_html", json!({"fileId":file}))["html"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(inline.contains(&format!("data:image/png;base64,{data}")));
    assert!(!inline.contains("/asset/"));
    let linked = edit(
        &mut s,
        "export_html",
        json!({"fileId":file,"assets":"files"}),
    )["html"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(linked.contains("file://") && linked.contains(&names[0]));

    // The saved file embeds its assets, so a copy opened in another library still renders.
    s.flush().unwrap();
    let path = s.path_of(&file).unwrap().clone();
    let raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["assets"][&names[0]], json!(data));
    let elsewhere = temp_dir("portable-copy");
    let copy = elsewhere.join("Design.cardboard");
    std::fs::copy(&path, &copy).unwrap();
    let mut other = Store::library(&elsewhere, None).unwrap();
    let id = edit(
        &mut other,
        "open_file",
        json!({"path":copy.to_string_lossy()}),
    )["fileId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(stored(&elsewhere), names);
    let exported = edit(&mut other, "export_html", json!({"fileId":id}))["html"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(exported, inline);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(elsewhere).unwrap();
}

#[test]
fn older_files_with_inline_images_migrate_on_open() {
    let dir = temp_dir("migrate");
    let mut doc = cardboard::document::Document::new("f-old".into(), "Old");
    let mut frame = cardboard::document::Node::new("node-2".into(), "div", "Frame");
    frame.styles.insert("width".into(), "100px".into());
    let mut img = cardboard::document::Node::new("node-3".into(), "img", "Image");
    let data = STANDARD.encode(png(4, 4, 3));
    img.attributes
        .insert("src".into(), format!("data:image/png;base64,{data}"));
    frame.children.push(img);
    doc.pages[0].artboards.push(frame);
    doc.next_id = 4;
    let path = dir.join("Old.cardboard");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({"format":"cardboard","version":2,"document":doc})).unwrap(),
    )
    .unwrap();
    let mut s = Store::library(&dir, None).unwrap();
    let id = edit(&mut s, "open_file", json!({"path":path.to_string_lossy()}))["fileId"]
        .as_str()
        .unwrap()
        .to_string();
    let src = s.file(&id).unwrap().node("node-3").unwrap().attributes["src"].clone();
    assert!(src.starts_with("/asset/"), "{src}");
    assert!(s.files[&id].dirty, "saved without an explicit edit");
    assert!(
        tools::call(&mut s, "undo", &json!({"fileId":id})).is_err(),
        "not an undo step"
    );
    s.flush().unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("\"assets\"") && !raw.contains("data:image"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn embedded_assets_must_match_their_hash() {
    let dir = temp_dir("tamper");
    let real = STANDARD.encode(png(1, 1, 4));
    let fake = format!("{}.png", "0".repeat(64));
    let doc = cardboard::document::Document::new("f-x".into(), "X");
    let path = dir.join("X.cardboard");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &json!({"format":"cardboard","version":2,"document":doc,"assets":{fake.clone():real}}),
        )
        .unwrap(),
    )
    .unwrap();
    store::read_document(&path).unwrap();
    assert!(stored(&dir).is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn memory_sessions_keep_working_without_a_store() {
    let mut s = Store::memory();
    let error = tools::call(
        &mut s,
        "import_image",
        &json!({"dataUri":format!("data:image/png;base64,{}", STANDARD.encode(png(2, 2, 5)))}),
    )
    .unwrap_err();
    assert!(error.contains("no asset store"), "{error}");
}

#[test]
fn image_sizes_and_types_come_from_content() {
    let gif = b"GIF89a\x20\x00\x10\x00".to_vec();
    assert_eq!(assets::image_size(&gif), Some((32, 16)));
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0];
    jpeg.extend([0xff, 0xc0, 0, 11, 8, 0, 50, 0, 70, 3, 0, 0]);
    assert_eq!(assets::image_size(&jpeg), Some((70, 50)));
    assert_eq!(
        assets::sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
        Some("svg")
    );
    assert_eq!(assets::sniff(b"wOF2...."), Some("woff2"));
    assert_eq!(assets::sniff(b"hello"), None);
    assert!(!assets::valid_name("../etc/passwd"));
}

#[test]
fn stock_results_keep_attribution() {
    let unsplash = stock::unsplash_result(&json!({
        "id":"abc","width":4000,"height":3000,"color":"#0c2340","alt_description":"boats in a harbor",
        "urls":{"regular":"https://images.unsplash.com/photo-1?w=1080","thumb":"https://images.unsplash.com/photo-1?w=200"},
        "links":{"html":"https://unsplash.com/photos/abc","download_location":"https://api.unsplash.com/photos/abc/download"},
        "user":{"name":"Ana Lima","links":{"html":"https://unsplash.com/@ana"}}
    }));
    assert_eq!(unsplash["stockId"], "unsplash:abc");
    assert_eq!(unsplash["attribution"], "Photo by Ana Lima on Unsplash");
    assert_eq!(
        unsplash["photographerUrl"],
        "https://unsplash.com/@ana?utm_source=cardboard&utm_medium=referral"
    );
    let openverse = stock::openverse_result(&json!({
        "id":"e945","title":"forest","url":"https://live.staticflickr.com/x.jpg","creator":"barnyz",
        "license":"by","license_version":"2.0","attribution":"\"forest\" by barnyz is licensed under CC BY 2.0."
    }));
    assert_eq!(openverse["stockId"], "openverse:e945");
    assert_eq!(openverse["license"], "CC BY 2.0");
    assert!(
        openverse["attribution"]
            .as_str()
            .unwrap()
            .contains("barnyz")
    );
    assert_eq!(stock::query_escape("red boat & sea"), "red+boat+%26+sea");
    if std::env::var("UNSPLASH_ACCESS_KEY").is_err() {
        let error =
            stock::search(&json!({"query":"boats","provider":"unsplash"}), None).unwrap_err();
        assert!(error.contains("UNSPLASH_ACCESS_KEY"), "{error}");
    }
}

/// The start of a TrueType file, enough to be recognized as a font.
fn ttf(salt: u8) -> Vec<u8> {
    let mut b = b"\0\x01\0\0\0\0\0\0\0\0\0\0".to_vec();
    b.push(salt);
    b
}

#[test]
fn local_fonts_become_document_font_faces() {
    let (dir, mut s, file, _) = library_with_file("fonts");
    let regular = dir.join("Brand-Regular.ttf");
    let bold = dir.join("Brand-Bold.ttf");
    std::fs::write(&regular, ttf(1)).unwrap();
    std::fs::write(&bold, ttf(2)).unwrap();
    edit(
        &mut s,
        "add_font",
        json!({"fileId":file,"family":"Brand Sans","path":regular.to_string_lossy()}),
    );
    let added = edit(
        &mut s,
        "add_font",
        json!({"fileId":file,"family":"Brand Sans","path":bold.to_string_lossy(),"weight":700}),
    );
    assert_eq!(
        added["fonts"],
        json!([{"family":"Brand Sans","weights":["400","700"],"styles":["normal"],"source":"file"}])
    );
    let info = edit(&mut s, "get_basic_info", json!({"fileId":file}));
    assert_eq!(info["fonts"][0]["family"], "Brand Sans");
    let listed = edit(
        &mut s,
        "list_fonts",
        json!({"fileId":file,"query":"zzzz-no-such-font"}),
    );
    assert_eq!(listed["document"][0]["family"], "Brand Sans");
    assert_eq!(listed["installed"], json!([]));

    // Re-adding a weight replaces it instead of duplicating.
    edit(
        &mut s,
        "add_font",
        json!({"fileId":file,"family":"Brand Sans","path":bold.to_string_lossy(),"weight":"700"}),
    );
    assert_eq!(s.file(&file).unwrap().fonts.len(), 2);

    // Export carries @font-face with the font inlined, so it renders anywhere.
    let html = edit(&mut s, "export_html", json!({"fileId":file}))["html"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(html.contains("@font-face{font-family:\"Brand Sans\";src:url(\"data:font/ttf;base64,"));
    assert!(html.contains("font-weight:700"));

    // Fonts travel inside the saved file like images.
    s.flush().unwrap();
    let raw: Value =
        serde_json::from_slice(&std::fs::read(s.path_of(&file).unwrap()).unwrap()).unwrap();
    assert_eq!(raw["assets"].as_object().unwrap().len(), 2);

    // Images are not fonts, bad names are rejected, removal is undoable.
    let png_path = dir.join("x.png");
    std::fs::write(&png_path, png(1, 1, 9)).unwrap();
    assert!(
        tools::call(
            &mut s,
            "add_font",
            &json!({"fileId":file,"family":"X","path":png_path.to_string_lossy()})
        )
        .is_err()
    );
    assert!(
        tools::call(
            &mut s,
            "add_font",
            &json!({"fileId":file,"family":"Bad\"}","path":bold.to_string_lossy()})
        )
        .is_err()
    );
    edit(
        &mut s,
        "remove_font",
        json!({"fileId":file,"family":"Brand Sans"}),
    );
    assert!(s.file(&file).unwrap().fonts.is_empty());
    edit(&mut s, "undo", json!({"fileId":file}));
    assert_eq!(s.file(&file).unwrap().fonts.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn google_fonts_requests_and_stylesheets_parse() {
    use cardboard::fonts;
    assert_eq!(
        fonts::google_url("Space Grotesk", &["700".into(), "400".into()], false).unwrap(),
        "https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@400;700&display=swap"
    );
    assert_eq!(
        fonts::google_url("Inter", &["400".into()], true).unwrap(),
        "https://fonts.googleapis.com/css2?family=Inter:ital,wght@0,400;1,400&display=swap"
    );
    assert!(fonts::google_url("Inter", &["heavy".into()], false).is_err());
    let css = "/* cyrillic */\n@font-face {\n  font-family: 'Inter';\n  font-style: normal;\n  font-weight: 400;\n  src: url(https://fonts.gstatic.com/a.woff2) format('woff2');\n  unicode-range: U+0400-045F;\n}\n/* latin */\n@font-face {\n  font-family: 'Inter';\n  font-style: italic;\n  font-weight: 700;\n  src: url(https://fonts.gstatic.com/b.woff2) format('woff2');\n  unicode-range: U+0000-00FF, U+0131;\n}\n";
    let latin = fonts::parse_google_css(css, &["latin".into()]);
    assert_eq!(
        latin,
        vec![(
            "700".to_string(),
            "italic".to_string(),
            Some("U+0000-00FF, U+0131".to_string()),
            "https://fonts.gstatic.com/b.woff2".to_string()
        )]
    );
    assert_eq!(fonts::parse_google_css(css, &["all".into()]).len(), 2);
}
