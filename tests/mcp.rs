use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use lilvault::{
    MAX_FILE_BYTES,
    config::{Agent, Auth, Config, Server, Vault},
    http,
    service::Service,
};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{fs, sync::Arc};
use tempfile::TempDir;
use tower::ServiceExt as _;

// Fixtures only; actual smoke/deployment credentials are generated randomly.
const WRITER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const READER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
struct Fixture {
    root: TempDir,
    app: Router,
}
impl Fixture {
    fn new() -> Self {
        Self::with_hosts(&["localhost", "127.0.0.1", "::1"])
    }

    fn with_hosts(hosts: &[&str]) -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("a/note.md"), "original").unwrap();
        fs::write(root.path().join("b/note.md"), "private b").unwrap();
        let config = Config {
            server: Server {
                mcp_allowed_hosts: hosts.iter().map(|host| (*host).to_owned()).collect(),
                ..Server::default()
            },
            auth: Auth::default(),
            vaults: ["a", "b"]
                .into_iter()
                .map(|id| Vault {
                    id: id.into(),
                    path: root.path().join(id),
                    enabled: true,
                })
                .collect(),
            agents: [
                ("writer", "WRITER", "a:read-write"),
                ("reader", "READER", "a:read"),
                ("other", "OTHER", "b:read-write"),
            ]
            .into_iter()
            .map(|(name, token_env, permission)| Agent {
                name: name.into(),
                token_env: token_env.into(),
                permissions: vec![permission.into()],
            })
            .collect(),
        };
        let service = Service::from_config(config, |key| match key {
            "WRITER" => Some(WRITER.into()),
            "READER" => Some(READER.into()),
            "OTHER" => Some(OTHER.into()),
            _ => None,
        })
        .unwrap();
        Self {
            root,
            app: http::router(Arc::new(service)),
        }
    }
    async fn rpc(&self, token: Option<&str>, headers: &[(&str, &str)], message: Value) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.app
            .clone()
            .oneshot(request.body(Body::from(message.to_string())).unwrap())
            .await
            .unwrap()
    }
    async fn call(&self, token: &str, name: &str, args: Value) -> Value {
        let response=self.rpc(Some(token),&[],json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}})).await;
        assert_eq!(response.status(), StatusCode::OK);
        decode(response).await
    }
}
async fn decode(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
fn tool_result(value: &Value) -> &Value {
    &value["result"]["structuredContent"]
}

#[tokio::test]
async fn sdk_handshake_and_discovery_are_authenticated_and_stateless() {
    let f = Fixture::new();
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}});
    assert_eq!(
        f.rpc(None, &[], init.clone()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = f.rpc(Some(WRITER), &[], init).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("mcp-session-id"));
    let value = decode(response).await;
    assert!(value["result"]["capabilities"]["tools"].is_object());
    assert_eq!(value["result"]["serverInfo"]["name"], "lilvault");
    assert_eq!(
        value["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    let response = f
        .rpc(
            Some(WRITER),
            &[],
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .await;
    let value = decode(response).await;
    let mut names = value["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        ["list_files", "list_vaults", "read_file", "write_file"]
    );
    // A forged old session header never bypasses fresh bearer authentication.
    assert_eq!(
        f.rpc(
            None,
            &[("mcp-session-id", "forged")],
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn every_call_uses_its_own_credential_and_shared_core_acl() {
    let f = Fixture::new();
    let a = f.call(WRITER, "list_vaults", json!({})).await;
    let b = f.call(OTHER, "list_vaults", json!({})).await;
    assert_eq!(
        tool_result(&a)["vaults"],
        json!([{"id":"a","access":"read-write"}])
    );
    assert_eq!(
        tool_result(&b)["vaults"],
        json!([{"id":"b","access":"read-write"}])
    );
    for id in ["b", "unknown"] {
        let value = f
            .call(WRITER, "read_file", json!({"vault_id":id,"path":"note.md"}))
            .await;
        assert_eq!(value["result"]["isError"], true);
        assert_eq!(tool_result(&value)["error"]["code"], "not_found");
    }
    let value = f
        .call(
            READER,
            "write_file",
            json!({"vault_id":"a","path":"new.md","content":"no","create_only":true}),
        )
        .await;
    assert_eq!(tool_result(&value)["error"]["code"], "forbidden");
    assert!(!f.root.path().join("a/new.md").exists());
}

#[tokio::test]
async fn tools_create_list_read_update_and_reject_blind_stale_or_unsafe_writes() {
    let f = Fixture::new();
    let value = f
        .call(
            WRITER,
            "write_file",
            json!({"vault_id":"a","path":"new.md","content":"hello","create_only":true}),
        )
        .await;
    assert_ne!(value["result"]["isError"], true);
    let revision = tool_result(&value)["revision"].as_str().unwrap().to_owned();
    assert_eq!(tool_result(&value)["cloud_sync"], "unknown");
    let value = f
        .call(WRITER, "read_file", json!({"vault_id":"a","path":"new.md"}))
        .await;
    assert_eq!(tool_result(&value)["content"], "hello");
    assert_eq!(tool_result(&value)["revision"], revision);
    let value = f.call(WRITER, "list_files", json!({"vault_id":"a"})).await;
    assert!(
        tool_result(&value)["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "new.md")
    );
    let value=f.call(WRITER,"write_file",json!({"vault_id":"a","path":"new.md","content":"updated","expected_revision":revision})).await;
    assert_ne!(value["result"]["isError"], true);
    let stale = f
        .call(
            WRITER,
            "write_file",
            json!({"vault_id":"a","path":"new.md","content":"stale","expected_revision":revision}),
        )
        .await;
    assert_eq!(tool_result(&stale)["error"]["code"], "precondition_failed");
    for (args, code) in [
        (
            json!({"vault_id":"a","path":"new.md","content":"blind"}),
            "precondition_required",
        ),
        (
            json!({"vault_id":"a","path":"new.md","content":"both","create_only":true,"expected_revision":revision}),
            "invalid_request",
        ),
        (
            json!({"vault_id":"a","path":"../outside.md","content":"bad","create_only":true}),
            "invalid_path",
        ),
        (
            json!({"vault_id":"a","path":".obsidian/no.md","content":"bad","create_only":true}),
            "invalid_path",
        ),
    ] {
        let value = f.call(WRITER, "write_file", args).await;
        assert_eq!(tool_result(&value)["error"]["code"], code);
    }
    assert_eq!(
        fs::read_to_string(f.root.path().join("a/new.md")).unwrap(),
        "updated"
    );
}

#[tokio::test]
async fn mcp_rejects_browser_origins_and_unapproved_hosts() {
    let f = Fixture::new();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    for origin in ["https://evil.example", "http://localhost:8737", "null"] {
        let response = f
            .rpc(Some(WRITER), &[("origin", origin)], request.clone())
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    // Separate request avoids a duplicate Host header; a single attacker Host is denied too.
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "evil.example")
        .header("authorization", format!("Bearer {WRITER}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Body::from(request.to_string()))
        .unwrap();
    assert_eq!(
        f.app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn explicitly_configured_host_is_required_for_remote_authorities() {
    let f = Fixture::with_hosts(&["approved.example:443"]);
    for (host, expected) in [
        ("approved.example:443", StatusCode::OK),
        ("evil.example", StatusCode::FORBIDDEN),
        ("127.0.0.1", StatusCode::FORBIDDEN),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", host)
            .header("authorization", format!("Bearer {WRITER}"))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18")
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
            ))
            .unwrap();
        assert_eq!(
            f.app.clone().oneshot(request).await.unwrap().status(),
            expected,
            "host={host}"
        );
    }
}

#[tokio::test]
async fn mcp_bounds_note_content_and_http_envelope() {
    let f = Fixture::new();
    let value=f.call(WRITER,"write_file",json!({"vault_id":"a","path":"big.md","content":"x".repeat(MAX_FILE_BYTES+1),"create_only":true})).await;
    assert_eq!(tool_result(&value)["error"]["code"], "too_large");
    assert!(!f.root.path().join("a/big.md").exists());
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "127.0.0.1")
        .header("authorization", format!("Bearer {WRITER}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Body::from(vec![b' '; MAX_FILE_BYTES * 6 + 64 * 1024 + 1]))
        .unwrap();
    assert_eq!(
        f.app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn real_sdk_client_initializes_and_calls_tools_over_http() {
    let f = Fixture::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, f.app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let config = StreamableHttpClientTransportConfig::with_uri(url).auth_header(WRITER);
    let client = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        ().serve(StreamableHttpClientTransport::from_config(config)),
    )
    .await
    .unwrap()
    .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 4);
    let value=client.call_tool(CallToolRequestParams::new("write_file").with_arguments(json!({"vault_id":"a","path":"sdk.md","content":"created by official SDK client","create_only":true}).as_object().unwrap().clone())).await.unwrap();
    assert_ne!(value.is_error, Some(true));
    assert_eq!(
        value.structured_content.as_ref().unwrap()["cloud_sync"],
        "unknown"
    );
    let read = client
        .call_tool(
            CallToolRequestParams::new("read_file").with_arguments(
                json!({"vault_id":"a","path":"sdk.md"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        read.structured_content.as_ref().unwrap()["content"],
        "created by official SDK client"
    );
    client.cancel().await.unwrap();
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    // f.root was not moved into the server task and remains alive through the calls.
    assert_eq!(
        fs::read_to_string(f.root.path().join("a/sdk.md")).unwrap(),
        "created by official SDK client"
    );
}
