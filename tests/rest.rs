use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use lilvault::{MAX_FILE_BYTES, config::Config, http, service::Service, vault::revision};
use serde_json::Value;
use std::{fs, os::unix::fs::symlink, sync::Arc};
use tempfile::TempDir;
use tower::ServiceExt;

// Deterministic credentials are fixtures only; never use them in a deployment.
const WRITER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const READER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct Harness {
    root: TempDir,
    app: Router,
}
impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("a/note.md"), "original").unwrap();
        fs::write(root.path().join("b/note.md"), "private b").unwrap();
        fs::create_dir(root.path().join("a/folder")).unwrap();
        fs::create_dir(root.path().join("a/.obsidian")).unwrap();
        fs::write(root.path().join("a/.obsidian/hidden.md"), "hidden fixture").unwrap();
        fs::write(root.path().join("a/image.png"), "not a note").unwrap();
        let text = format!(
            r#"
[[vaults]]
id = "a"
path = "{}"
[[vaults]]
id = "b"
path = "{}"
[[agents]]
name = "writer"
token_env = "WRITER"
permissions = ["a:read-write"]
[[agents]]
name = "reader"
token_env = "READER"
permissions = ["a:read"]
[[agents]]
name = "other"
token_env = "OTHER"
permissions = ["b:read-write"]
"#,
            root.path().join("a").display(),
            root.path().join("b").display()
        );
        let service = Service::from_config(Config::parse(&text).unwrap(), |key| match key {
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

    async fn request(
        &self,
        method: &str,
        uri: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
        body: impl Into<Body>,
    ) -> Response {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.app
            .clone()
            .oneshot(request.body(body.into()).unwrap())
            .await
            .unwrap()
    }
    async fn get(&self, path: &str, token: &str) -> Response {
        self.request("GET", path, Some(token), &[], Body::empty())
            .await
    }
    async fn put(&self, path: &str, token: &str, header: (&str, &str), content: &str) -> Response {
        self.request("PUT", path, Some(token), &[header], content.to_owned())
            .await
    }
}
async fn body(response: Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}
async fn json(response: Response) -> Value {
    serde_json::from_slice(&body(response).await).unwrap()
}

#[tokio::test]
async fn health_and_authentication_are_separate() {
    let h = Harness::new();
    for path in ["/healthz", "/readyz"] {
        let r = h.request("GET", path, None, &[], Body::empty()).await;
        assert_eq!(r.status(), StatusCode::OK);
    }
    for token in [None, Some("wrong")] {
        let r = h
            .request("GET", "/v1/vaults", token, &[], Body::empty())
            .await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(r.headers()["www-authenticate"], "Bearer");
        assert_eq!(json(r).await["error"]["code"], "unauthorized");
    }
    let r = h
        .request(
            "GET",
            "/v1/vaults",
            Some(WRITER),
            &[("authorization", READER)],
            Body::empty(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn vaults_are_isolated_and_read_only_agents_cannot_write() {
    let h = Harness::new();
    let list = json(h.get("/v1/vaults", WRITER).await).await;
    assert_eq!(
        list["vaults"],
        serde_json::json!([{"id":"a","access":"read-write"}])
    );
    for id in ["b", "unknown"] {
        let r = h
            .get(&format!("/v1/vaults/{id}/files/note.md"), WRITER)
            .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(
        body(h.get("/v1/vaults/b/files/note.md", OTHER).await).await,
        b"private b"
    );
    let r = h
        .put(
            "/v1/vaults/a/files/new.md",
            READER,
            ("if-none-match", "*"),
            "no",
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body(h.get("/v1/vaults/a/files/note.md", READER).await).await,
        b"original"
    );
}

#[tokio::test]
async fn listing_filters_internal_files_attachments_and_links() {
    let h = Harness::new();
    symlink(h.root.path().join("b"), h.root.path().join("a/link")).unwrap();
    let value = json(h.get("/v1/vaults/a/files", WRITER).await).await;
    assert_eq!(
        value["files"],
        serde_json::json!([
            {"path":"folder","type":"directory","size":0},
            {"path":"note.md","type":"file","size":8}
        ])
    );
    assert_eq!(
        json(h.get("/v1/vaults/a/files?path=folder", WRITER).await).await["files"],
        serde_json::json!([])
    );
}

#[tokio::test]
async fn create_read_and_revision_update_use_content_hashes() {
    let h = Harness::new();
    let path = "/v1/vaults/a/files/folder/new.md";
    let r = h.put(path, WRITER, ("if-none-match", "*"), "hello").await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let etag = r.headers()["etag"].to_str().unwrap().to_owned();
    assert_eq!(etag, format!("\"{}\"", revision(b"hello")));
    let value = json(r).await;
    assert_eq!(value["local_write"], true);
    assert_eq!(value["cloud_sync"], "unknown");
    let r = h.get(path, WRITER).await;
    assert_eq!(r.headers()["etag"], etag);
    assert_eq!(r.headers()["content-type"], "text/markdown; charset=utf-8");
    assert_eq!(body(r).await, b"hello");
    assert_eq!(
        h.put(path, WRITER, ("if-none-match", "*"), "blind")
            .await
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
    let r = h.put(path, WRITER, ("if-match", &etag), "updated").await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_ne!(r.headers()["etag"], etag);
    assert_eq!(
        h.put(path, WRITER, ("if-match", &etag), "stale")
            .await
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(body(h.get(path, WRITER).await).await, b"updated");
    assert_eq!(
        fs::read_dir(h.root.path().join("a/folder"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn preconditions_are_required_and_strict() {
    let h = Harness::new();
    let path = "/v1/vaults/a/files/new.md";
    assert_eq!(
        h.request("PUT", path, Some(WRITER), &[], "no")
            .await
            .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    for headers in [
        vec![("if-match", "*")],
        vec![("if-none-match", "bad")],
        vec![("if-none-match", "*"), ("if-match", "bad")],
        vec![("if-none-match", "*"), ("if-none-match", "*")],
    ] {
        let r = h.request("PUT", path, Some(WRITER), &headers, "no").await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(r).await["error"]["code"], "invalid_request");
    }
    let etag = format!("\"{}\"", revision(b"absent"));
    assert_eq!(
        h.put(path, WRITER, ("if-match", &etag), "no")
            .await
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
}

#[tokio::test]
async fn only_one_concurrent_writer_can_replace_a_revision() {
    let h = Harness::new();
    let path = "/v1/vaults/a/files/note.md";
    let etag = format!("\"{}\"", revision(b"original"));
    let (a, b) = tokio::join!(
        h.put(path, WRITER, ("if-match", &etag), "first"),
        h.put(path, WRITER, ("if-match", &etag), "second")
    );
    let mut statuses = [a.status().as_u16(), b.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 412]);
    let value = body(h.get(path, WRITER).await).await;
    assert!(value == b"first" || value == b"second");
}

#[tokio::test]
async fn rejects_encoded_traversal_hidden_paths_and_non_markdown() {
    let h = Harness::new();
    for path in [
        "../b/note.md",
        "%2e%2e/b/note.md",
        "%252e%252e/b/note.md",
        "%2Fetc/passwd.md",
        "folder/%2e%2e/note.md",
        ".obsidian/hidden.md",
        "folder//note.md",
        "folder%5Cnote.md",
        "note.md%00",
        "image.png",
        "folder/.hidden.md",
    ] {
        let r = h.get(&format!("/v1/vaults/a/files/{path}"), WRITER).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "path={path}");
        assert_eq!(json(r).await["error"]["code"], "invalid_path");
    }
    assert_eq!(
        h.get("/v1/vaults/a/files?path=%2e%2e", WRITER)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.get("/v1/vaults/a/files?unknown=1", WRITER).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.get("/v1/vaults/a/files/missing.md", WRITER)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn rejects_symlinks_parent_symlinks_and_hard_links() {
    let h = Harness::new();
    let a = h.root.path().join("a");
    let b = h.root.path().join("b");
    symlink(b.join("note.md"), a.join("link.md")).unwrap();
    symlink(&b, a.join("parent")).unwrap();
    fs::hard_link(b.join("note.md"), a.join("hard.md")).unwrap();
    for path in ["link.md", "parent/note.md", "hard.md"] {
        let uri = format!("/v1/vaults/a/files/{path}");
        assert_eq!(h.get(&uri, WRITER).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            h.put(&uri, WRITER, ("if-none-match", "*"), "no")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(fs::read_to_string(b.join("note.md")).unwrap(), "private b");
}

#[tokio::test]
async fn limits_bytes_and_rejects_invalid_utf8_without_writing() {
    let h = Harness::new();
    let path = "/v1/vaults/a/files/new.md";
    let r = h
        .request(
            "PUT",
            path,
            Some(WRITER),
            &[("if-none-match", "*")],
            vec![b'x'; MAX_FILE_BYTES + 1],
        )
        .await;
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(r).await["error"]["code"], "too_large");
    let r = h
        .request(
            "PUT",
            path,
            Some(WRITER),
            &[("if-none-match", "*")],
            vec![0xff],
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(!h.root.path().join("a/new.md").exists());
    fs::write(
        h.root.path().join("a/big.md"),
        vec![b'x'; MAX_FILE_BYTES + 1],
    )
    .unwrap();
    assert_eq!(
        h.get("/v1/vaults/a/files/big.md", WRITER).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    fs::write(h.root.path().join("a/bad.md"), [0xff]).unwrap();
    assert_eq!(
        h.get("/v1/vaults/a/files/bad.md", WRITER).await.status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn unavailable_replaced_vault_fails_closed() {
    let h = Harness::new();
    fs::rename(h.root.path().join("a"), h.root.path().join("old-a")).unwrap();
    fs::create_dir(h.root.path().join("a")).unwrap();
    assert_eq!(
        h.get("/readyz", WRITER).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        h.get("/v1/vaults/a/files/note.md", WRITER).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        h.put(
            "/v1/vaults/a/files/new.md",
            WRITER,
            ("if-none-match", "*"),
            "no"
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(!h.root.path().join("a/new.md").exists());
    assert_eq!(h.get("/healthz", WRITER).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn routing_errors_are_json_and_delete_is_not_implemented_yet() {
    let h = Harness::new();
    for (method, path, status) in [
        ("GET", "/nope", StatusCode::NOT_FOUND),
        // DELETE exists but still requires an exact precondition header.
        (
            "DELETE",
            "/v1/vaults/a/files/note.md",
            StatusCode::PRECONDITION_REQUIRED,
        ),
    ] {
        let r = h
            .request(method, path, Some(WRITER), &[], Body::empty())
            .await;
        assert_eq!(r.status(), status);
        assert!(json(r).await["error"]["code"].is_string());
    }
}

#[tokio::test]
async fn delete_requires_exact_revision_and_enforces_permissions() {
    let h = Harness::new();
    let etag = h.get("/v1/vaults/a/files/note.md", WRITER).await.headers()["etag"]
        .to_str()
        .unwrap()
        .to_owned();
    // Deletion needs an exact If-Match; create-style and missing preconditions are rejected.
    assert_eq!(
        h.request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(WRITER),
            &[("if-none-match", "*")],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(WRITER),
            &[],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let stale = format!("\"{}\"", revision(b"not-current"));
    assert_eq!(
        h.request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(WRITER),
            &[("if-match", stale.as_str())],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::PRECONDITION_FAILED
    );
    assert!(h.root.path().join("a/note.md").exists());
    // Read-only credentials and other-vault callers cannot delete.
    assert_eq!(
        h.request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(READER),
            &[("if-match", etag.as_str())],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(OTHER),
            &[("if-match", etag.as_str())],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let r = h
        .request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(WRITER),
            &[("if-match", etag.as_str())],
            Body::empty(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(json(r).await["deleted"], true);
    assert!(!h.root.path().join("a/note.md").exists());
    assert_eq!(
        h.get("/v1/vaults/a/files/note.md", WRITER).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.request(
            "DELETE",
            "/v1/vaults/a/files/note.md",
            Some(WRITER),
            &[("if-match", etag.as_str())],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn search_finds_filenames_and_content_within_authorized_vault() {
    let h = Harness::new();
    fs::write(
        h.root.path().join("a/folder/coffee.md"),
        "I enjoy Espresso daily",
    )
    .unwrap();
    fs::write(h.root.path().join("a/folder/tea.md"), "only water here").unwrap();
    fs::create_dir(h.root.path().join("a/secret-folder")).unwrap();
    fs::write(
        h.root.path().join("a/secret-folder/hidden.md"),
        "espresso secrets",
    )
    .unwrap();
    let value = json(h.get("/v1/vaults/a/search?q=espresso", WRITER).await).await;
    let matches = value["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    let paths: Vec<&str> = matches
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"folder/coffee.md"));
    assert!(paths.contains(&"secret-folder/hidden.md"));
    let coffee = matches
        .iter()
        .find(|m| m["path"] == "folder/coffee.md")
        .unwrap();
    assert!(coffee["snippet"].as_str().unwrap().contains("espresso"));
    // Filename matches carry no snippet; internal .obsidian files stay
    // excluded while a visible file with the same stem is returned.
    let value = json(h.get("/v1/vaults/a/search?q=coffee", WRITER).await).await;
    assert_eq!(
        value["matches"],
        serde_json::json!([{"path":"folder/coffee.md","snippet":""}])
    );
    let value = json(h.get("/v1/vaults/a/search?q=hidden", WRITER).await).await;
    assert_eq!(
        value["matches"],
        serde_json::json!([{"path":"secret-folder/hidden.md","snippet":""}])
    );
    let value = json(h.get("/v1/vaults/a/search?q=fixture", WRITER).await).await;
    assert_eq!(value["matches"], serde_json::json!([]));
    // Authorization applies before results: other vaults and strangers see 404.
    assert_eq!(
        h.get("/v1/vaults/b/search?q=private", WRITER)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.get("/v1/vaults/b/search?q=private", OTHER).await.status(),
        StatusCode::OK
    );
    // Bounds and empty queries fail closed.
    for uri in [
        "/v1/vaults/a/search",
        "/v1/vaults/a/search?q=",
        "/v1/vaults/a/search?q=x&limit=0",
        "/v1/vaults/a/search?q=x&limit=101",
    ] {
        let r = h.get(uri, WRITER).await;
        assert!(
            r.status() == StatusCode::BAD_REQUEST || r.status() == StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
    let r = h
        .get("/v1/vaults/a/search?q=x&limit=1&extra=1", WRITER)
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    // Limit is honored for content matches.
    let value = json(
        h.get("/v1/vaults/a/search?q=espresso&limit=1", WRITER)
            .await,
    )
    .await;
    assert_eq!(value["matches"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn status_reports_local_availability_and_unknown_sync() {
    let h = Harness::new();
    let value = json(h.get("/v1/vaults/a/status", WRITER).await).await;
    assert_eq!(value["vault_id"], "a");
    assert_eq!(value["access"], "read-write");
    assert_eq!(value["local_available"], true);
    assert_eq!(value["sync"], "unknown");
    assert_eq!(
        h.get("/v1/vaults/b/status", WRITER).await.status(),
        StatusCode::NOT_FOUND
    );
    let value = json(h.get("/v1/vaults/b/status", OTHER).await).await;
    assert_eq!(value["access"], "read-write");
}
