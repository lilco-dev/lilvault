use crate::{
    MAX_FILE_BYTES,
    error::Error,
    service::{Agent, Service},
    vault::{Condition, valid_revision},
};
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Path, Query, Request, State,
        rejection::{BytesRejection, PathRejection, QueryRejection},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::json;
use std::{sync::Arc, time::Duration};

type FilePath = Result<Path<(String, String)>, PathRejection>;

pub fn router(service: Arc<Service>) -> Router {
    let api = Router::new()
        .route("/vaults", get(vaults))
        .route("/vaults/{vault}/files", get(list))
        .route("/vaults/{vault}/files/{*path}", get(read).put(write))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            authenticate,
        ));
    Router::new()
        .route("/healthz", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/readyz", get(ready))
        .nest("/v1", api)
        .fallback(|| async { Error::NotFound })
        .method_not_allowed_fallback(|| async {
            (
                StatusCode::METHOD_NOT_ALLOWED,
                Json(
                    json!({"error":{"code":"method_not_allowed","message":"method not supported"}}),
                ),
            )
        })
        .layer(DefaultBodyLimit::max(MAX_FILE_BYTES))
        .layer(middleware::from_fn(deadline))
        .with_state(service)
}

async fn authenticate(
    State(service): State<Arc<Service>>,
    mut request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let agent = if headers.get_all(header::AUTHORIZATION).iter().count() == 1 {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| service.authenticate(token))
    } else {
        None
    };
    let Some(agent) = agent else {
        return Error::Unauthorized.into_response();
    };
    request.extensions_mut().insert(agent);
    next.run(request).await
}

async fn deadline(request: Request, next: Next) -> Response {
    match tokio::time::timeout(Duration::from_secs(30), next.run(request)).await {
        Ok(response) => response,
        Err(_) => Error::Timeout.into_response(),
    }
}

async fn work<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| Error::Unavailable)?
}

async fn ready(State(service): State<Arc<Service>>) -> Result<Response, Error> {
    let ready = work(move || Ok(service.ready())).await?;
    Ok((
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({"ready":ready,"sync":"unknown"})),
    )
        .into_response())
}

async fn vaults(
    State(service): State<Arc<Service>>,
    Extension(agent): Extension<Agent>,
) -> Json<serde_json::Value> {
    Json(json!({"vaults":service.list_vaults(&agent)}))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    #[serde(default)]
    path: String,
}

async fn list(
    State(service): State<Arc<Service>>,
    Extension(agent): Extension<Agent>,
    vault: Result<Path<String>, PathRejection>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<serde_json::Value>, Error> {
    let Path(vault) = vault.map_err(|_| Error::BadPath)?;
    let Query(query) = query.map_err(|_| Error::BadRequest)?;
    let entries = work(move || service.list(&agent, &vault, &query.path)).await?;
    Ok(Json(json!({"files":entries})))
}

async fn read(
    State(service): State<Arc<Service>>,
    Extension(agent): Extension<Agent>,
    path: FilePath,
) -> Result<Response, Error> {
    let Path((vault, path)) = path.map_err(|_| Error::BadPath)?;
    let note = work(move || service.read(&agent, &vault, &path)).await?;
    let mut response = note.content.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::ETAG,
        format!("\"{}\"", note.revision)
            .parse()
            .map_err(|_| Error::Unavailable)?,
    );
    Ok(response)
}

fn precondition(headers: &HeaderMap) -> Result<Condition, Error> {
    for name in [header::IF_MATCH, header::IF_NONE_MATCH] {
        if headers.get_all(name).iter().count() > 1 {
            return Err(Error::BadRequest);
        }
    }
    match (
        headers.get(header::IF_MATCH),
        headers.get(header::IF_NONE_MATCH),
    ) {
        (None, None) => Err(Error::PreconditionRequired),
        (None, Some(value)) if value == "*" => Ok(Condition::Create),
        (Some(value), None) => {
            let value = value.to_str().map_err(|_| Error::BadRequest)?;
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .ok_or(Error::BadRequest)?;
            if !valid_revision(value) {
                return Err(Error::BadRequest);
            }
            Ok(Condition::Match(value.to_owned()))
        }
        _ => Err(Error::BadRequest),
    }
}

async fn write(
    State(service): State<Arc<Service>>,
    Extension(agent): Extension<Agent>,
    path: FilePath,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, Error> {
    let Path((vault, path)) = path.map_err(|_| Error::BadPath)?;
    let condition = precondition(&headers)?;
    let created = matches!(condition, Condition::Create);
    let bytes = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            Error::TooLarge
        } else {
            Error::BadRequest
        }
    })?;
    let content = String::from_utf8(bytes.to_vec()).map_err(|_| Error::BadRequest)?;
    let revision = work(move || service.write(&agent, &vault, &path, &content, &condition)).await?;
    let etag = format!("\"{revision}\"")
        .parse::<HeaderValue>()
        .map_err(|_| Error::Unavailable)?;
    let mut response = (
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!({"revision":revision,"local_write":true,"cloud_sync":"unknown"})),
    )
        .into_response();
    response.headers_mut().insert(header::ETAG, etag);
    Ok(response)
}
