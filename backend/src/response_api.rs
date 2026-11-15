use crate::{
    response_store::StoredResponse,
    server::{AppState, ProtocolCredential, error, extract_credential},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, net::IpAddr};

fn response_id(id: &str) -> String {
    id.strip_prefix("resp_").unwrap_or(id).to_string()
}
fn with_response_id(mut value: Value) -> Value {
    if let Some(id) = value.get("id").and_then(Value::as_str).map(str::to_owned)
        && !id.starts_with("resp_")
    {
        value["id"] = format!("resp_{id}").into();
    }
    value
}
fn terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "incomplete")
}

pub async fn create_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: IpAddr,
    body: Value,
) -> Response {
    let token = match extract_credential(&headers, ProtocolCredential::OpenAI, None) {
        Ok(token) => token,
        Err(status) => return error(status, "invalid or missing API credentials"),
    };
    let client = match state.store.authenticate_client(&token).await {
        Ok(Some(client)) => client,
        _ => return error(StatusCode::UNAUTHORIZED, "invalid API key"),
    };
    let root = match body.as_object() {
        Some(root) => root.clone(),
        None => return error(StatusCode::BAD_REQUEST, "request must be a JSON object"),
    };
    let previous = root.get("previous_response_id").and_then(Value::as_str);
    let conversation = root.get("conversation").and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("id").and_then(Value::as_str))
    });
    if previous.is_some() && conversation.is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "previous_response_id and conversation are mutually exclusive",
        );
    }
    let mut history = Vec::new();
    let mut model = root
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if let Some(id) = previous {
        match state.store.response(client.id, &response_id(id)).await {
            Ok(Some(previous)) => {
                if previous.response_json.is_none()
                    || terminal(&previous.status) && previous.status != "completed"
                {
                    return error(StatusCode::BAD_REQUEST, "previous response is not complete");
                }
                history.extend(previous.input_items);
                history.extend(previous.output_items);
                if model.is_empty() {
                    model = previous.model;
                }
            }
            Ok(None) => return error(StatusCode::NOT_FOUND, "previous response not found"),
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "response state unavailable",
                );
            }
        }
    }
    if let Some(id) = conversation {
        let mut conversation_items = Vec::new();
        let mut after = None;
        loop {
            match state
                .store
                .conversation_items(client.id, id, 100, false, after.as_deref())
                .await
            {
                Ok(Some(items)) => {
                    if items.is_empty() {
                        break;
                    }
                    after = items
                        .last()
                        .and_then(|item| item.get("id").and_then(Value::as_str))
                        .map(str::to_owned);
                    let count = items.len();
                    conversation_items.extend(items);
                    if count < 100 {
                        break;
                    }
                }
                Ok(None) => return error(StatusCode::NOT_FOUND, "conversation not found"),
                Err(_) => {
                    return error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "conversation state unavailable",
                    );
                }
            }
        }
        history.extend(conversation_items);
    }
    let background = root
        .get("background")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut body = body;
    if !history.is_empty() {
        let new_items = body
            .get("input")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        history.extend(new_items);
        body["input"] = Value::Array(history);
        body.as_object_mut().unwrap().remove("previous_response_id");
        body.as_object_mut().unwrap().remove("conversation");
        if let Some(instructions) = root.get("instructions") {
            body["instructions"] = instructions.clone();
        }
    }
    let store = root.get("store").and_then(Value::as_bool).unwrap_or(true);
    if previous.is_some() || conversation.is_some() {
        body["previous_response_id"] = previous.map(|id| json!(id)).unwrap_or(Value::Null);
        body["conversation"] = conversation
            .map(|id| json!({"id":id}))
            .unwrap_or(Value::Null);
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let id_for_task = id.clone();
    let background_inbound = crate::server::InboundProtocol::OpenAIResponses;
    let background_state = state.clone();
    let background_headers = headers.clone();
    let background_body = body.clone();
    let background_peer = peer;
    let task_client = client.clone();
    if store || background {
        let input = match root.get("input") {
            Some(Value::String(input)) => vec![
                json!({"type":"message","role":"user","content":[{"type":"input_text","text":input}]}),
            ],
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        };
        if state
            .store
            .create_response(
                client.id,
                &response_id(&id),
                &model,
                background,
                conversation,
                previous,
                &body,
                &input,
            )
            .await
            .is_err()
        {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "response state unavailable",
            );
        }
    }
    if background {
        tokio::spawn(async move {
            let _ = background_state
                .store
                .response_status(task_client.id, &response_id(&id_for_task), "in_progress")
                .await;
            if let Some(stored) = background_state
                .store
                .response(task_client.id, &response_id(&id_for_task))
                .await
                .ok()
                .flatten()
            {
                let mut progress = placeholder(&stored);
                progress["status"] = "in_progress".into();
                let _ = background_state
                    .store
                    .update_response_json(
                        task_client.id,
                        &response_id(&id_for_task),
                        "in_progress",
                        &progress,
                    )
                    .await;
            }
            let response = crate::server::relay(
                background_state.clone(),
                background_headers,
                background_inbound,
                background_peer,
                background_body,
                Some(crate::server::ResponseLifecycle {
                    client_key: task_client.id,
                    id: id_for_task.clone(),
                }),
            )
            .await;
            let (parts, body) = response.into_parts();
            if !parts.status.is_success()
                && background_state
                    .store
                    .response(task_client.id, &response_id(&id_for_task))
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|stored| !terminal(&stored.status))
            {
                let _ = background_state
                    .store
                    .response_status(task_client.id, &response_id(&id_for_task), "failed")
                    .await;
            }
            let _ = body;
        });
        let value = json!({"id":format!("resp_{id}"),"object":"response","created_at":0,"status":"queued","output":[],"model":model,"background":true,"store":store,"previous_response_id":previous,"conversation":conversation.map(|id| json!({"id":id})),"error":null,"incomplete_details":null,"usage":null,"tools":root.get("tools").cloned().unwrap_or(json!([])),"tool_choice":root.get("tool_choice").cloned().unwrap_or(json!("auto")),"parallel_tool_calls":root.get("parallel_tool_calls").cloned().unwrap_or(json!(true))});
        return Json(value).into_response();
    }
    crate::server::relay(
        state,
        headers,
        crate::server::InboundProtocol::OpenAIResponses,
        peer,
        body,
        Some(crate::server::ResponseLifecycle {
            client_key: client.id,
            id,
        }),
    )
    .await
}

pub async fn get_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let stored = match state.store.response(client.id, &response_id(&id)).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return error(StatusCode::NOT_FOUND, "response not found"),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "response state unavailable",
            );
        }
    };
    if stored.request_json.get("store").and_then(Value::as_bool) == Some(false)
        && !stored.background
    {
        return error(StatusCode::NOT_FOUND, "response not found");
    }
    if query
        .get("stream")
        .and_then(|value| value.parse::<bool>().ok())
        == Some(true)
    {
        let after = query
            .get("starting_after")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let body = stored
            .stream_events
            .iter()
            .skip(after)
            .map(|event| {
                format!(
                    "event: {}\ndata: {}\n\n",
                    event
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("message"),
                    event
                )
            })
            .collect::<String>();
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(body))
            .unwrap();
    }
    match stored.response_json {
        Some(value) => Json(with_response_id(value)).into_response(),
        None => Json(placeholder(&stored)).into_response(),
    }
}
pub async fn cancel_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let stored = match state.store.response(client.id, &response_id(&id)).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return error(StatusCode::NOT_FOUND, "response not found"),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "response state unavailable",
            );
        }
    };
    if !stored.background {
        return error(
            StatusCode::BAD_REQUEST,
            "only background responses can be cancelled",
        );
    }
    if !terminal(&stored.status)
        && state
            .store
            .request_response_cancel(client.id, &response_id(&id))
            .await
            .unwrap_or(false)
    { /* Cancellation is observed by the task. */ }
    let mut value = stored
        .response_json
        .clone()
        .unwrap_or_else(|| placeholder(&stored));
    value["status"] = "cancelled".into();
    let _ = state
        .store
        .update_response(
            client.id,
            &response_id(&id),
            "cancelled",
            &value,
            &stored.output_items,
        )
        .await;
    Json(with_response_id(value)).into_response()
}
pub async fn input_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let stored = match state.store.response(client.id, &response_id(&id)).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return error(StatusCode::NOT_FOUND, "response not found"),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "response state unavailable",
            );
        }
    };
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 100);
    let descending = query
        .get("order")
        .map(|order| order != "asc")
        .unwrap_or(true);
    let mut items = stored.input_items;
    if descending {
        items.reverse();
    }
    if let Some(after) = query.get("after")
        && let Some(position) = items
            .iter()
            .position(|item| item.get("id").and_then(Value::as_str) == Some(after))
    {
        items = items.into_iter().skip(position + 1).collect();
    }
    let has_more = items.len() > limit;
    let data: Vec<_> = items.into_iter().take(limit).collect();
    Json(json!({"object":"list","data":data,"first_id":data.first().and_then(|item| item.get("id")).cloned().unwrap_or(Value::Null),"last_id":data.last().and_then(|item| item.get("id")).cloned().unwrap_or(Value::Null),"has_more":has_more})).into_response()
}
pub async fn create_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.len() > 20 {
        return error(
            StatusCode::BAD_REQUEST,
            "a maximum of 20 items can be created",
        );
    }
    match state
        .store
        .create_conversation(
            client.id,
            body.get("metadata").unwrap_or(&json!({})),
            &items,
        )
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation state unavailable",
        ),
    }
}
pub async fn get_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    match state.store.conversation(client.id, &id).await {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "conversation not found"),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation state unavailable",
        ),
    }
}
pub async fn update_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    match state
        .store
        .update_conversation(client.id, &id, body.get("metadata").unwrap_or(&json!({})))
        .await
    {
        Ok(true) => get_conversation(State(state), headers, Path(id)).await,
        Ok(false) => error(StatusCode::NOT_FOUND, "conversation not found"),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation state unavailable",
        ),
    }
}
pub async fn delete_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    match state.store.delete_conversation(client.id, &id).await {
        Ok(true) => {
            Json(json!({"id":id,"object":"conversation.deleted","deleted":true})).into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "conversation not found"),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation state unavailable",
        ),
    }
}
pub async fn add_conversation_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let items = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() || items.len() > 20 {
        return error(
            StatusCode::BAD_REQUEST,
            "between 1 and 20 items are required",
        );
    }
    match state
        .store
        .append_conversation_items(client.id, &id, &items)
        .await
    {
        Ok(()) => Json(json!({"object":"list","data":items,"has_more":false})).into_response(),
        Err(_) => error(StatusCode::NOT_FOUND, "conversation not found"),
    }
}
pub async fn list_conversation_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(20);
    let descending = query
        .get("order")
        .map(|order| order != "asc")
        .unwrap_or(true);
    match state
        .store
        .conversation_items(
            client.id,
            &id,
            limit,
            descending,
            query.get("after").map(String::as_str),
        )
        .await
    {
        Ok(Some(data)) => {
            Json(json!({"object":"list","data":data,"has_more":false})).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "conversation not found"),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation state unavailable",
        ),
    }
}
pub async fn delete_conversation_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, item)): Path<(String, String)>,
) -> Response {
    let Some(client) = authenticate(&state, &headers).await else {
        return error(StatusCode::UNAUTHORIZED, "invalid API key");
    };
    match state
        .store
        .delete_conversation_item(client.id, &id, &item)
        .await
    {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "conversation item not found"),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation state unavailable",
        ),
    }
}
async fn authenticate(state: &AppState, headers: &HeaderMap) -> Option<crate::ClientKey> {
    let token = extract_credential(headers, ProtocolCredential::OpenAI, None).ok()?;
    state.store.authenticate_client(&token).await.ok().flatten()
}
fn placeholder(stored: &StoredResponse) -> Value {
    json!({"id":format!("resp_{}",stored.id),"object":"response","created_at":0,"status":stored.status,"output":stored.output_items,"model":stored.model,"background":stored.background,"store":true,"previous_response_id":stored.previous_response_id,"conversation":stored.conversation_id.as_ref().map(|id| json!({"id":id})),"error":if stored.status == "failed" { json!({"code":"server_error","message":"Response execution failed"}) } else { Value::Null },"incomplete_details":null,"usage":null})
}
pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/v1/responses/{id}", get(get_response))
        .route("/v1/responses/{id}/cancel", post(cancel_response))
        .route("/v1/responses/{id}/input_items", get(input_items))
        .route("/v1/conversations", post(create_conversation))
        .route(
            "/v1/conversations/{id}",
            get(get_conversation)
                .post(update_conversation)
                .delete(delete_conversation),
        )
        .route(
            "/v1/conversations/{id}/items",
            post(add_conversation_items).get(list_conversation_items),
        )
        .route(
            "/v1/conversations/{id}/items/{item}",
            delete(delete_conversation_item),
        )
}
