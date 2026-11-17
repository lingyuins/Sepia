use sepia_gateway::protocol::{
    ChatRequest, ChatResponse, FinishReason, Message, Protocol, Role, StreamEvent, decode_request,
    decode_response, decode_sse, encode_request, encode_response, encode_sse,
};
use serde_json::{Value, json};

fn request(protocol: Protocol) -> Value {
    match protocol {
        Protocol::OpenAI => {
            json!({"model":"alias","messages":[{"role":"system","content":"Be concise"},{"role":"user","content":"Hello"}],"max_tokens":64,"temperature":0.2,"stream":false,"tools":[]})
        }
        Protocol::Anthropic => {
            json!({"model":"alias","system":"Be concise","messages":[{"role":"user","content":[{"type":"text","text":"Hello"}]}],"max_tokens":64,"temperature":0.2,"stream":false})
        }
        Protocol::Gemini => {
            json!({"systemInstruction":{"parts":[{"text":"Be concise"}]},"contents":[{"role":"user","parts":[{"text":"Hello"}]}],"generationConfig":{"maxOutputTokens":64,"temperature":0.2}})
        }
        Protocol::OpenAIResponses => panic!("outbound-only protocol has no inbound request"),
    }
}

fn response(protocol: Protocol) -> Value {
    match protocol {
        Protocol::OpenAI => {
            json!({"id":"upstream-id","model":"example","choices":[{"index":0,"message":{"role":"assistant","content":"Hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1}})
        }
        Protocol::Anthropic => {
            json!({"id":"upstream-id","model":"example","role":"assistant","content":[{"type":"text","text":"Hi"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":1}})
        }
        Protocol::Gemini => {
            json!({"candidates":[{"content":{"role":"model","parts":[{"text":"Hi"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":1}})
        }
        Protocol::OpenAIResponses => {
            json!({"id":"upstream-id","object":"response","status":"completed","model":"example","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hi"}]}],"usage":{"input_tokens":3,"output_tokens":1}})
        }
    }
}

#[test]
fn native_and_cross_protocol_json_preserves_supported_text_semantics() {
    for inbound in [Protocol::OpenAI, Protocol::Anthropic, Protocol::Gemini] {
        let decoded = decode_request(inbound, request(inbound), Some("alias")).unwrap();
        assert_eq!(decoded.system.as_deref(), Some("Be concise"));
        assert_eq!(decoded.messages, vec![Message::new(Role::User, "Hello")]);
        assert_eq!(decoded.max_tokens, Some(64));
        for outbound in [Protocol::OpenAI, Protocol::Anthropic, Protocol::Gemini] {
            let wire = encode_request(outbound, &decoded, "example").unwrap();
            let reparsed = decode_request(outbound, wire, Some("example")).unwrap();
            assert_eq!(reparsed.system, decoded.system);
            assert_eq!(reparsed.messages, decoded.messages);
            assert_eq!(reparsed.max_tokens, decoded.max_tokens);
            let reply = decode_response(outbound, response(outbound)).unwrap();
            let encoded = encode_response(inbound, &reply);
            let final_reply = decode_response(inbound, encoded).unwrap();
            assert_eq!(final_reply.text, "Hi");
            assert_eq!(final_reply.input_tokens, 3);
            assert_eq!(final_reply.output_tokens, 1);
            assert_eq!(final_reply.finish_reason, FinishReason::Stop);
        }
    }
}

#[test]
fn stream_events_round_trip_with_native_event_names_and_done_markers() {
    let events = vec![
        StreamEvent::Start {
            id: "id".into(),
            model: "example".into(),
        },
        StreamEvent::Delta("Hello".into()),
        StreamEvent::Usage {
            input_tokens: 2,
            output_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
        },
        StreamEvent::Stop(FinishReason::Stop),
    ];
    for protocol in [Protocol::OpenAI, Protocol::Anthropic, Protocol::Gemini] {
        let wire = encode_sse(protocol, &events);
        let decoded = decode_sse(protocol, &wire).unwrap();
        if protocol == Protocol::Gemini {
            assert_eq!(
                decoded,
                vec![
                    StreamEvent::Start {
                        id: String::new(),
                        model: String::new()
                    },
                    StreamEvent::Delta("Hello".into()),
                    StreamEvent::Usage {
                        input_tokens: 2,
                        output_tokens: 1,
                        cached_tokens: 0,
                        reasoning_tokens: 0,
                    },
                    StreamEvent::Stop(FinishReason::Stop)
                ]
            );
        } else {
            assert_eq!(decoded, events);
        }
        match protocol {
            Protocol::OpenAI => assert!(wire.contains("data: [DONE]\n\n")),
            Protocol::Anthropic => assert!(wire.contains("event: message_stop\n")),
            Protocol::Gemini => assert!(wire.contains("\"candidates\"")),
            Protocol::OpenAIResponses => unreachable!("not part of this loop"),
        }
    }
}

#[test]
fn responses_provider_encodes_chat_requests_and_decodes_completed_responses() {
    let decoded =
        decode_request(Protocol::OpenAI, request(Protocol::OpenAI), Some("alias")).unwrap();
    let wire = encode_request(Protocol::OpenAIResponses, &decoded, "example").unwrap();
    assert_eq!(wire["model"], "example");
    assert_eq!(wire["instructions"], "Be concise");
    assert_eq!(wire["max_output_tokens"], 64);
    assert_eq!(wire["store"], false);
    assert_eq!(wire["input"][0]["role"], "user");
    assert_eq!(wire["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(wire["input"][0]["content"][0]["text"], "Hello");
    let reply = decode_response(
        Protocol::OpenAIResponses,
        response(Protocol::OpenAIResponses),
    )
    .unwrap();
    assert_eq!(reply.text, "Hi");
    assert_eq!(reply.input_tokens, 3);
    assert_eq!(reply.output_tokens, 1);
    assert_eq!(reply.finish_reason, FinishReason::Stop);
    assert_eq!(
        encode_response(Protocol::OpenAI, &reply)["choices"][0]["message"]["content"],
        "Hi"
    );
    assert!(decode_request(Protocol::OpenAIResponses, wire.clone(), None).is_err());
    assert!(
        decode_response(
            Protocol::OpenAIResponses,
            json!({"id":"resp_1","status":"in_progress","output":[]})
        )
        .is_err()
    );
}

#[test]
fn responses_stream_events_decode_to_shared_events() {
    let wire = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"example\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Hello\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"example\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}}\n\n"
    );
    let decoded = decode_sse(Protocol::OpenAIResponses, wire).unwrap();
    assert_eq!(
        decoded,
        vec![
            StreamEvent::Start {
                id: "resp_1".into(),
                model: "example".into()
            },
            StreamEvent::Delta("Hello".into()),
            StreamEvent::Usage {
                input_tokens: 2,
                output_tokens: 1,
                cached_tokens: 0,
                reasoning_tokens: 0,
            },
            StreamEvent::Stop(FinishReason::Stop),
        ]
    );
}

#[test]
fn accepts_tools_and_media_but_rejects_unsupported_multiplicity() {
    let tools = json!({"model":"x","messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"weather"}}]});
    assert!(decode_request(Protocol::OpenAI, tools, None).is_ok());
    let multiple = json!({"model":"x","messages":[{"role":"user","content":"hello"}],"n":2});
    assert!(decode_request(Protocol::OpenAI, multiple, None).is_err());
    let media = json!({"model":"x","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.test/x"}}]}]});
    assert!(decode_request(Protocol::OpenAI, media, None).is_ok());
    assert!(decode_request(Protocol::OpenAI, json!({"model":"x","messages":[]}), None).is_err());
    assert_eq!(
        ChatRequest {
            model: "x".into(),
            ..ChatRequest::default()
        }
        .model,
        "x"
    );
    let empty = ChatResponse::default();
    assert_eq!(empty.text, "");
}
