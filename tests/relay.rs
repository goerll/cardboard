use cardboard::{mcp, relay::Session};
use serde_json::{Value, json};

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

#[test]
fn the_relay_replays_the_handshake_and_answers_abandoned_requests() {
    let mut s = Session::new("r-1".into());
    // Nothing to replay before the agent finished its own handshake.
    assert!(s.handshake().is_empty());
    let init = json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"agent","version":"1"}}});
    let (line, id) = s.outgoing(&init.to_string());
    assert_eq!(id.as_deref(), Some("0"));
    let sent = parse(&line);
    assert_eq!(
        sent["params"]["capabilities"]["experimental"][mcp::SESSION_CAPABILITY]["id"],
        "r-1"
    );
    assert!(
        s.incoming(json!({"jsonrpc":"2.0","id":0,"result":{}}).to_string())
            .is_some()
    );
    s.outgoing(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string());
    s.outgoing(
        &json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"undo"}}).to_string(),
    );
    s.outgoing(&json!({"jsonrpc":"2.0","id":"x","method":"tools/list"}).to_string());

    // The connection drops: in-flight requests get answers the agent can recover from.
    let mut answers: Vec<Value> = s.abandon().iter().map(|l| parse(l)).collect();
    answers.sort_by_key(|a| a["id"].is_string());
    assert_eq!(answers[0]["id"], 7);
    assert_eq!(answers[0]["result"]["isError"], true);
    assert_eq!(answers[1]["id"], "x");
    assert!(answers[1]["error"]["message"].is_string());
    assert!(s.abandon().is_empty());

    // Reconnected: the same session is initialized again and its reply is swallowed.
    let replay = s.handshake();
    assert_eq!(replay.len(), 2);
    let again = parse(&replay[0]);
    assert_eq!(again["method"], "initialize");
    assert_eq!(
        again["params"]["capabilities"]["experimental"][mcp::SESSION_CAPABILITY]["id"],
        "r-1"
    );
    let reply = json!({"jsonrpc":"2.0","id":again["id"],"result":{}}).to_string();
    assert!(s.incoming(reply).is_none());
    assert!(
        s.incoming(
            json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}).to_string()
        )
        .is_some()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_app_takes_the_library_over_from_a_headless_host() {
    use cardboard::{relay::Host, store};
    let dir = std::env::temp_dir().join(format!("cardboard-test-{}", store::random_hex(6)));
    let library = dir.join("library");
    std::fs::create_dir_all(&library).unwrap();
    // A non-empty library never imports a legacy workspace from the real home directory.
    let existing = cardboard::document::Document::new("f-seed".into(), "Seed");
    store::write_document(&library.join("Seed.cardboard"), &existing).unwrap();
    let socket = dir.join("cardboard.sock");

    let host = Host::start(&socket, &library).await.unwrap();
    let hub = host.hub().clone();
    let file = hub.call("create_file", &json!({"name":"Draft"})).unwrap();
    let path = std::path::PathBuf::from(file["path"].as_str().unwrap());
    hub.call(
        "create_artboard",
        &json!({"fileId":file["fileId"],"name":"Unsaved","styles":{"width":"1px","height":"1px"}}),
    )
    .unwrap();

    // A second host can't start while one is live.
    let second = Host::start(&socket, &library).await;
    assert_eq!(
        second.err().map(|e| e.kind()),
        Some(std::io::ErrorKind::AddrInUse)
    );
    let status = tokio::task::spawn_blocking({
        let socket = socket.clone();
        move || mcp::control(&socket, &json!({"cardboard":"status"}))
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status["mode"], "headless");

    // The app binds: the host saves everything first and stops writing.
    let listener = tokio::task::spawn_blocking({
        let socket = socket.clone();
        move || mcp::bind_socket(&socket)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(hub.is_retired());
    assert!(std::fs::read_to_string(&path).unwrap().contains("Unsaved"));
    assert!(hub.call("create_page", &json!({})).is_err());
    // Shutting the retired host down must not remove the app's socket.
    host.shutdown();
    assert!(socket.exists());
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doctor_finds_cardboard_in_claude_code_config() {
    let config = json!({
        "mcpServers": {"other": {"command": "node", "args": ["x"]}},
        "projects": {"/work": {"mcpServers": {"design": {"command": "/opt/bin/cardboard", "args": ["mcp"]}}}}
    });
    let found = cardboard::doctor::cardboard_servers(&config);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].1, "/opt/bin/cardboard");
    assert_eq!(found[0].2, vec!["mcp".to_string()]);
}
