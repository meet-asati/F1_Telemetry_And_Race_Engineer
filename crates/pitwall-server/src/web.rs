//! HTTP + WebSocket API and static UI hosting.

use crate::actors::{CommsCmd, HealthInfo, Hub};
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use race_state::db::Db;
use race_state::snapshot::{ClientCommand, ServerMessage, SCHEMA_VERSION};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::sync::{broadcast, mpsc};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub comms: mpsc::Sender<CommsCmd>,
    pub db: Option<Db>,
    pub token: Option<String>,
}

pub fn router(state: AppState, ui_dir: PathBuf) -> Router {
    let api = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/health", get(health))
        .route("/api/snapshot", get(snapshot))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/current", get(current_session))
        .route("/api/sessions/{id}/laps", get(session_laps))
        .route("/api/sessions/{id}/radio", get(session_radio))
        .route("/api/track-outline/{track_id}", get(track_outline).delete(delete_outline))
        .route("/api/radio", post(post_radio))
        .with_state(state);
    let index = ui_dir.join("index.html");
    if index.exists() {
        api.fallback_service(ServeDir::new(&ui_dir).fallback(ServeFile::new(index)))
    } else {
        tracing::warn!("UI not built ({} missing) — run `npm run build` in ui/. API only.", index.display());
        api.fallback(|| async { (StatusCode::NOT_FOUND, "Pit wall UI not built. Run `npm install && npm run build` in ui/.") })
    }
}

fn authorized(state: &AppState, provided: Option<&str>) -> bool {
    match &state.token {
        None => true,
        Some(t) => provided == Some(t.as_str()),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| client_loop(socket, state))
}

async fn client_loop(socket: WebSocket, state: AppState) {
    let (mut tx, mut rx) = socket.split();
    let hello = ServerMessage::Hello { schema_version: SCHEMA_VERSION, server_version: env!("CARGO_PKG_VERSION").into(), auth_required: state.token.is_some() };
    if tx.send(Message::Text(serde_json::to_string(&hello).unwrap_or_default().into())).await.is_err() {
        return;
    }
    let mut snaps = state.hub.snapshot_json.clone();
    let mut events = state.hub.events.subscribe();
    loop {
        tokio::select! {
            changed = snaps.changed() => {
                if changed.is_err() { break; }
                let msg = snaps.borrow_and_update().clone();
                if tx.send(Message::Text(msg)).await.is_err() { break; }
            }
            ev = events.recv() => match ev {
                Ok(msg) => { if tx.send(Message::Text(msg)).await.is_err() { break; } }
                Err(broadcast::error::RecvError::Lagged(n)) => tracing::debug!("ws client lagged {n} events"),
                Err(_) => break,
            },
            incoming = rx.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let Message::Text(text) = msg else { continue };
                let reply = handle_command(&state, &text).await;
                if let Some(err) = reply {
                    let m = ServerMessage::Error { message: err };
                    if tx.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&m).unwrap_or_default()))).await.is_err() { break; }
                }
            }
        }
    }
}

async fn handle_command(state: &AppState, text: &str) -> Option<String> {
    let cmd: ClientCommand = match serde_json::from_str(text) {
        Ok(c) => c,
        Err(e) => return Some(format!("bad command: {e}")),
    };
    if cmd.is_mutating() && !authorized(state, cmd.token()) {
        return Some("unauthorized: missing or wrong token".into());
    }
    if state.comms.send(CommsCmd::Client(cmd)).await.is_err() {
        return Some("comms unavailable".into());
    }
    None
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let h: HealthInfo = state.hub.health.borrow().clone();
    Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "schemaVersion": SCHEMA_VERSION,
        "authRequired": state.token.is_some(),
        "currentSessionId": *state.hub.current_session.borrow(),
        "status": h,
    }))
}

async fn snapshot(State(state): State<AppState>) -> Response {
    let snap = state.hub.snapshot.borrow().clone();
    Json(&*snap).into_response()
}

#[allow(clippy::result_large_err)]
fn db_or_503(state: &AppState) -> Result<&Db, Response> {
    state.db.as_ref().ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "database disabled").into_response())
}

fn internal(e: anyhow::Error) -> Response {
    tracing::warn!("api error: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
}

async fn sessions(State(state): State<AppState>) -> Response {
    let db = match db_or_503(&state) {
        Ok(d) => d,
        Err(r) => return r,
    };
    match db.list_sessions(50).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

async fn current_session(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "id": *state.hub.current_session.borrow() }))
}

async fn session_laps(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let db = match db_or_503(&state) {
        Ok(d) => d,
        Err(r) => return r,
    };
    match db.laps(id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

async fn session_radio(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let db = match db_or_503(&state) {
        Ok(d) => d,
        Err(r) => return r,
    };
    match db.radio_events(id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

async fn track_outline(State(state): State<AppState>, Path(track_id): Path<i64>) -> Response {
    let db = match db_or_503(&state) {
        Ok(d) => d,
        Err(r) => return r,
    };
    match db.outline(track_id).await {
        Ok(Some(o)) => Json(o).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no outline recorded for this track yet").into_response(),
        Err(e) => internal(e),
    }
}

async fn delete_outline(State(state): State<AppState>, headers: HeaderMap, Path(track_id): Path<i64>) -> Response {
    if !authorized(&state, header_token(&headers)) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let db = match db_or_503(&state) {
        Ok(d) => d,
        Err(r) => return r,
    };
    match db.delete_outline(track_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct RadioBody {
    text: String,
    #[serde(default)]
    attachments: Vec<String>,
    token: Option<String>,
}

fn header_token(headers: &HeaderMap) -> Option<&str> {
    headers.get("x-pitwall-token").and_then(|v| v.to_str().ok())
}

async fn post_radio(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<RadioBody>) -> Response {
    let token = body.token.as_deref().or_else(|| header_token(&headers));
    if !authorized(&state, token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    if body.text.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "text is empty").into_response();
    }
    let cmd = ClientCommand::RadioSend { text: body.text, attachments: body.attachments, token: None };
    match state.comms.send(CommsCmd::Client(cmd)).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "comms unavailable").into_response(),
    }
}
