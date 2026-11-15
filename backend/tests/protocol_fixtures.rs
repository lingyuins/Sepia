use sepia_gateway::protocol::{
    FinishReason, Protocol, StreamEvent, decode_request, decode_response, decode_sse, encode_sse,
};
use serde_json::{Value, json};

fn value(input: &str) -> Value {
    serde_json::from_str(input).unwrap()
}

fn payloads(wire: &str) -> Vec<Value> {
    wire.split("\n\n")
        .filter_map(|block| {
            let data = block
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .collect::<Vec<_>>()
                .join("\n");
            (!data.is_empty()).then(|| serde_json::from_str(&data).unwrap())
        })
        .collect()
}

const OPENAI_JSON: &str = r#"{"id":"chatcmpl-B9sVn0cQ","object":"chat.completion","created":1741470000,"model":"gpt-4o-2024-08-06","choices":[{"index":0,"message":{"role":"assistant","content":"Hello from OpenAI"},"logprobs":null,"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":4,"total_tokens":13}}"#;

const ANTHROPIC_JSON: &str = r#"{"id":"msg_01X5fP3y","type":"message","role":"assistant","model":"claude-3-5-sonnet-20241022","content":[{"type":"text","text":"Hello from Anthropic"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":5}}"#;

const GEMINI_JSON: &str = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"Hello from Gemini"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3,"totalTokenCount":10},"modelVersion":"gemini-2.0-flash"}"#;

const ANTHROPIC_SSE: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_01X5fP3y","type":"message","role":"assistant","model":"claude-3-5-sonnet-20241022","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: ping
data: {"type":"ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"!"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}

event: message_stop
data: {"type":"message_stop"}
"#;

const OPENAI_SSE: &str = r#"data: {"id":"chatcmpl-B9sVn0cQ","object":"chat.completion.chunk","created":1741470000,"model":"gpt-4o-2024-08-06","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-B9sVn0cQ","object":"chat.completion.chunk","created":1741470000,"model":"gpt-4o-2024-08-06","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-B9sVn0cQ","object":"chat.completion.chunk","created":1741470000,"model":"gpt-4o-2024-08-06","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":null}

data: {"id":"chatcmpl-B9sVn0cQ","object":"chat.completion.chunk","created":1741470000,"model":"gpt-4o-2024-08-06","choices":[],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}

data: [DONE]

"#;

#[test]
fn decodes_captured_native_json_responses() {
    let openai = decode_response(Protocol::OpenAI, value(OPENAI_JSON)).unwrap();
    assert_eq!(openai.text, "Hello from OpenAI");
    assert_eq!(openai.input_tokens, 9);
    assert_eq!(openai.output_tokens, 4);
    assert_eq!(openai.finish_reason, FinishReason::Stop);

    let anthropic = decode_response(Protocol::Anthropic, value(ANTHROPIC_JSON)).unwrap();
    assert_eq!(anthropic.text, "Hello from Anthropic");
    assert_eq!(anthropic.input_tokens, 12);
    assert_eq!(anthropic.output_tokens, 5);
    assert_eq!(anthropic.finish_reason, FinishReason::Stop);

    let gemini = decode_response(Protocol::Gemini, value(GEMINI_JSON)).unwrap();
    assert_eq!(gemini.text, "Hello from Gemini");
    assert_eq!(gemini.input_tokens, 7);
    assert_eq!(gemini.output_tokens, 3);
    assert_eq!(gemini.finish_reason, FinishReason::Stop);
}

#[test]
fn anthropic_stream_uses_message_start_usage_and_cumulative_message_delta_usage() {
    let events = decode_sse(Protocol::Anthropic, ANTHROPIC_SSE).unwrap();
    assert_eq!(
        events,
        vec![
            StreamEvent::Start {
                id: "msg_01X5fP3y".into(),
                model: "claude-3-5-sonnet-20241022".into(),
            },
            StreamEvent::Delta("Hello".into()),
            StreamEvent::Delta("!".into()),
            StreamEvent::Usage {
                input_tokens: 25,
                output_tokens: 15,
            },
            StreamEvent::Stop(FinishReason::Stop),
        ]
    );
}

#[test]
fn anthropic_encoding_has_required_start_usage_and_single_content_block_lifecycle() {
    let events = vec![
        StreamEvent::Start {
            id: "msg_out".into(),
            model: "claude-3-5-sonnet-20241022".into(),
        },
        StreamEvent::Delta("Hello".into()),
        StreamEvent::Delta("!".into()),
        StreamEvent::Usage {
            input_tokens: 25,
            output_tokens: 15,
        },
        StreamEvent::Stop(FinishReason::Stop),
    ];
    let wire = encode_sse(Protocol::Anthropic, &events);
    assert_eq!(wire.matches("event: message_start\n").count(), 1);
    assert_eq!(wire.matches("event: content_block_start\n").count(), 1);
    assert_eq!(wire.matches("event: content_block_delta\n").count(), 2);
    assert_eq!(wire.matches("event: content_block_stop\n").count(), 1);
    assert_eq!(wire.matches("event: message_delta\n").count(), 1);
    assert_eq!(wire.matches("event: message_stop\n").count(), 1);

    let payloads = payloads(&wire);
    let types = payloads
        .iter()
        .map(|payload| payload["type"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        types,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ]
    );
    assert_eq!(
        payloads[0]["message"]["usage"]["input_tokens"].as_u64(),
        Some(25)
    );
    assert_eq!(
        payloads[0]["message"]["usage"]["output_tokens"].as_u64(),
        Some(15)
    );
    assert_eq!(decode_sse(Protocol::Anthropic, &wire).unwrap(), events);
}

#[test]
fn anthropic_message_start_always_contains_usage() {
    let wire = encode_sse(
        Protocol::Anthropic,
        &[StreamEvent::Start {
            id: "msg_out".into(),
            model: "claude-3-5-sonnet-20241022".into(),
        }],
    );
    let payloads = payloads(&wire);
    assert_eq!(payloads[0]["type"].as_str(), Some("message_start"));
    assert!(payloads[0]["message"]["usage"].is_object());
    assert_eq!(
        payloads[0]["message"]["usage"]["input_tokens"].as_u64(),
        Some(0)
    );
    assert_eq!(
        payloads[0]["message"]["usage"]["output_tokens"].as_u64(),
        Some(0)
    );
}

#[test]
fn anthropic_incremental_encoding_starts_and_stops_the_text_block_once() {
    let start = encode_sse(
        Protocol::Anthropic,
        &[StreamEvent::Start {
            id: "msg_out".into(),
            model: "claude-3-5-sonnet-20241022".into(),
        }],
    );
    let first = encode_sse(Protocol::Anthropic, &[StreamEvent::Delta("Hello".into())]);
    let second = encode_sse(Protocol::Anthropic, &[StreamEvent::Delta("!".into())]);
    let stop = encode_sse(
        Protocol::Anthropic,
        &[StreamEvent::Stop(FinishReason::Stop)],
    );
    let wire = format!("{start}{first}{second}{stop}");
    assert_eq!(wire.matches("event: content_block_start\n").count(), 1);
    assert_eq!(wire.matches("event: content_block_delta\n").count(), 2);
    assert_eq!(wire.matches("event: content_block_stop\n").count(), 1);
    assert_eq!(wire.matches("event: message_stop\n").count(), 1);
}

#[test]
fn openai_usage_null_is_ignored_and_native_usage_is_preserved() {
    let response = decode_response(
        Protocol::OpenAI,
        value(r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"Hi"},"finish_reason":"stop"}],"usage":null}"#),
    )
    .unwrap();
    assert_eq!(response.text, "Hi");
    assert_eq!(response.input_tokens, 0);
    assert_eq!(response.output_tokens, 0);

    let events = decode_sse(Protocol::OpenAI, OPENAI_SSE).unwrap();
    assert_eq!(
        events,
        vec![
            StreamEvent::Start {
                id: "chatcmpl-B9sVn0cQ".into(),
                model: "gpt-4o-2024-08-06".into(),
            },
            StreamEvent::Delta("Hi".into()),
            StreamEvent::Stop(FinishReason::Stop),
            StreamEvent::Usage {
                input_tokens: 7,
                output_tokens: 2,
            },
        ]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, StreamEvent::Usage { .. }))
            .count(),
        1
    );
}

#[test]
fn gemini_accepts_usage_only_and_prompt_feedback_without_fabricated_text() {
    let usage_only = decode_response(
        Protocol::Gemini,
        value(r#"{"usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":0,"totalTokenCount":11}}"#),
    )
    .unwrap();
    assert_eq!(usage_only.text, "");
    assert_eq!(usage_only.input_tokens, 11);
    assert_eq!(usage_only.output_tokens, 0);

    let blocked = decode_response(
        Protocol::Gemini,
        value(r#"{"promptFeedback":{"blockReason":"SAFETY","safetyRatings":[{"category":"HARM_CATEGORY_DANGEROUS_CONTENT","probability":"HIGH"}]}}"#),
    )
    .unwrap();
    assert_eq!(blocked.text, "");
    assert_eq!(blocked.finish_reason, FinishReason::Other);

    let empty = decode_response(Protocol::Gemini, value(r#"{"candidates":[]}"#)).unwrap();
    assert_eq!(empty.text, "");
    assert_eq!(empty.input_tokens, 0);
    assert_eq!(empty.output_tokens, 0);

    let usage_events = decode_sse(
        Protocol::Gemini,
        "data: {\"usageMetadata\":{\"promptTokenCount\":11,\"candidatesTokenCount\":4,\"totalTokenCount\":15}}\n\n",
    )
    .unwrap();
    assert_eq!(
        usage_events,
        vec![
            StreamEvent::Start {
                id: String::new(),
                model: String::new(),
            },
            StreamEvent::Usage {
                input_tokens: 11,
                output_tokens: 4,
            },
        ]
    );

    let blocked_events = decode_sse(
        Protocol::Gemini,
        "data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"}}\n\n",
    )
    .unwrap();
    assert_eq!(
        blocked_events,
        vec![
            StreamEvent::Start {
                id: String::new(),
                model: String::new(),
            },
            StreamEvent::Stop(FinishReason::Other),
        ]
    );
}

#[test]
fn accepts_tool_media_and_rejects_multiple_upstream_results() {
    assert!(decode_response(Protocol::OpenAI, value(r#"{"choices":[{},{}]}"#),).is_err());
    assert!(
        decode_response(
            Protocol::OpenAI,
            value(r#"{"choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#),
        )
        .is_ok()
    );
    assert!(
        decode_response(
            Protocol::OpenAI,
            value(r#"{"choices":[{"index":0,"message":{"role":"assistant","content":[{"type":"image_url","image_url":{"url":"https://example.test/a.png"}}]},"finish_reason":"stop"}]}"#),
        )
        .is_ok()
    );
    assert!(decode_sse(Protocol::OpenAI, "data: {\"choices\":[{},{}]}\n\n",).is_err());
    assert!(
        decode_sse(
            Protocol::OpenAI,
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"weather\",\"arguments\":\"{}\"}}]}}]}\n\n",
        )
        .is_ok()
    );

    assert!(
        decode_response(
            Protocol::Anthropic,
            value(r#"{"id":"msg_1","content":[{"type":"tool_use","id":"toolu_1","name":"weather","input":{}}],"stop_reason":"tool_use","usage":{"input_tokens":3,"output_tokens":1}}"#),
        )
        .is_ok()
    );
    assert!(
        decode_sse(
            Protocol::Anthropic,
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"weather\",\"input\":{}}}\n\n",
        )
        .is_ok()
    );
    assert!(
        decode_sse(
            Protocol::Anthropic,
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
        )
        .is_ok()
    );

    assert!(
        decode_response(
            Protocol::Gemini,
            value(r#"{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"weather","args":{}}}]},"finishReason":"STOP"}]}"#),
        )
        .is_ok()
    );
    assert!(
        decode_response(
            Protocol::Gemini,
            value(r#"{"candidates":[{"content":{"role":"model","parts":[{"inlineData":{"mimeType":"image/png","data":"AAAA"}}]},"finishReason":"STOP"}]}"#),
        )
        .is_ok()
    );
    assert!(
        decode_response(
            Protocol::Gemini,
            value(r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"one"}]},"finishReason":"STOP"},{"content":{"role":"model","parts":[{"text":"two"}]},"finishReason":"STOP"}]}"#),
        )
        .is_err()
    );
    assert!(
        decode_sse(
            Protocol::Gemini,
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"weather\",\"args\":{}}}]},\"finishReason\":\"STOP\"}]}\n\n",
        )
        .is_ok()
    );
}

#[test]
fn null_optional_unsupported_fields_are_ignored() {
    let openai = decode_request(
        Protocol::OpenAI,
        value(
            r#"{"model":"alias","messages":[{"role":"user","content":"hello"}],"n":null,"stream":null,"stop":null,"tools":null,"response_format":null,"logprobs":null,"top_logprobs":null,"reasoning_effort":null,"tool_choice":null,"functions":null}"#,
        ),
        None,
    )
    .unwrap();
    assert!(!openai.stream);
    assert_eq!(openai.messages.len(), 1);

    let anthropic = decode_request(
        Protocol::Anthropic,
        value(
            r#"{"model":"alias","max_tokens":16,"messages":[{"role":"user","content":"hello"}],"thinking":null,"tools":null,"stream":null,"system":null}"#,
        ),
        None,
    )
    .unwrap();
    assert!(!anthropic.stream);
    assert_eq!(anthropic.system, None);

    let gemini = decode_request(
        Protocol::Gemini,
        value(
            r#"{"contents":[{"role":"user","parts":[{"text":"hello"}]}],"safetySettings":null,"tools":null,"generationConfig":{"candidateCount":null,"thinkingConfig":null,"maxOutputTokens":null,"temperature":null}}"#,
        ),
        Some("gemini-2.0-flash"),
    )
    .unwrap();
    assert!(!gemini.stream);
    assert_eq!(gemini.max_tokens, None);
}

#[test]
fn malformed_present_optional_values_are_rejected() {
    for request in [
        json!({"model":"alias","messages":[{"role":"user","content":"hello"}],"n":"2"}),
        json!({"model":"alias","messages":[{"role":"user","content":"hello"}],"n":2}),
        json!({"model":"alias","messages":[{"role":"user","content":"hello"}],"stream":"true"}),
    ] {
        assert!(decode_request(Protocol::OpenAI, request, None).is_err());
    }
    assert!(
        decode_request(
            Protocol::Anthropic,
            json!({"model":"alias","max_tokens":16,"messages":[{"role":"user","content":"hello"}],"stream":1}),
            None,
        )
        .is_err()
    );
    for request in [
        json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}],"generationConfig":{"candidateCount":"2"}}),
        json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}],"generationConfig":{"candidateCount":2}}),
    ] {
        assert!(decode_request(Protocol::Gemini, request, Some("gemini-2.0-flash")).is_err());
    }
}
