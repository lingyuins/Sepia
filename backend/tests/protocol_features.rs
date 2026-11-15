use sepia_gateway::protocol::{
    ContentBlock, Protocol, StreamEvent, ToolChoice, decode_request, decode_response, decode_sse,
    encode_request, encode_response, encode_sse,
};
use serde_json::{Value, json};

#[test]
fn function_tools_and_media_survive_every_provider_mapping() {
    let weather = json!({"type":"function","function":{"name":"weather","description":"Read weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}});
    for inbound in [Protocol::OpenAI, Protocol::Anthropic, Protocol::Gemini] {
        let input = match inbound {
            Protocol::OpenAI => json!({
                "model":"alias","messages":[
                    {"role":"user","content":[
                        {"type":"text","text":"Weather?"},
                        {"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8="}}
                    ]},
                    {"role":"assistant","tool_calls":[{"id":"call_weather","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},
                    {"role":"tool","tool_call_id":"call_weather","content":"sunny"}
                ],"tools":[weather],"tool_choice":"auto","max_tokens":128
            }),
            Protocol::Anthropic => json!({
                "model":"alias","messages":[
                    {"role":"user","content":[
                        {"type":"text","text":"Weather?"},
                        {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}
                    ]},
                    {"role":"assistant","content":[{"type":"tool_use","id":"call_weather","name":"weather","input":{"city":"Paris"}}]},
                    {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_weather","content":"sunny"}]}
                ],"tools":[{"name":"weather","description":"Read weather","input_schema":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}],"tool_choice":{"type":"auto"},"max_tokens":128
            }),
            Protocol::Gemini => json!({
                "contents":[
                    {"role":"user","parts":[{"text":"Weather?"},{"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}]},
                    {"role":"model","parts":[{"functionCall":{"name":"weather","args":{"city":"Paris"},"id":"call_weather"}}]},
                    {"role":"user","parts":[{"functionResponse":{"name":"weather","response":{"result":"sunny"},"id":"call_weather"}}]}
                ],"tools":[{"functionDeclarations":[{"name":"weather","description":"Read weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}]}],"toolConfig":{"functionCallingConfig":{"mode":"AUTO"}},"generationConfig":{"maxOutputTokens":128}
            }),
        };
        let request = decode_request(inbound, input, Some("alias")).unwrap();
        assert_eq!(request.messages.len(), 3, "{inbound:?}");
        assert_eq!(request.tools.len(), 1, "{inbound:?}");
        assert_eq!(request.tools[0].name, "weather");
        assert_eq!(request.tool_choice, ToolChoice::Auto);
        assert!(matches!(
            request.messages[0].content[1],
            ContentBlock::Image { .. }
        ));
        for outbound in [Protocol::OpenAI, Protocol::Anthropic, Protocol::Gemini] {
            let wire = encode_request(outbound, &request, "upstream").unwrap();
            let reparsed = decode_request(outbound, wire.clone(), Some("upstream")).unwrap();
            let normalize = |messages: &[sepia_gateway::protocol::Message]| {
                messages
                    .iter()
                    .map(|message| {
                        message
                            .content
                            .iter()
                            .map(|block| match block {
                                ContentBlock::ToolResult {
                                    call_id,
                                    content,
                                    is_error,
                                    ..
                                } => ContentBlock::ToolResult {
                                    call_id: call_id.clone(),
                                    name: None,
                                    content: content.clone(),
                                    is_error: *is_error,
                                },
                                block => block.clone(),
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                normalize(&reparsed.messages),
                normalize(&request.messages),
                "{inbound:?} -> {outbound:?}: {wire}"
            );
            assert_eq!(reparsed.tools, request.tools);
            assert_eq!(reparsed.tool_choice, request.tool_choice);
        }
    }
}

#[test]
fn function_call_json_and_streaming_preserve_ids_arguments_and_finish_reason() {
    for protocol in [Protocol::OpenAI, Protocol::Anthropic, Protocol::Gemini] {
        let response = match protocol {
            Protocol::OpenAI => {
                json!({"id":"chat_1","model":"upstream","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_weather","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2}})
            }
            Protocol::Anthropic => {
                json!({"id":"msg_1","model":"upstream","content":[{"type":"tool_use","id":"call_weather","name":"weather","input":{"city":"Paris"}}],"stop_reason":"tool_use","usage":{"input_tokens":4,"output_tokens":2}})
            }
            Protocol::Gemini => {
                json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"weather","args":{"city":"Paris"},"id":"call_weather"}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2}})
            }
        };
        let decoded = decode_response(protocol, response).unwrap();
        assert_eq!(decoded.content.len(), 1);
        let ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } = &decoded.content[0]
        else {
            panic!("missing tool call")
        };
        assert_eq!(id, "call_weather");
        assert_eq!(name, "weather");
        assert_eq!(arguments, "{\"city\":\"Paris\"}");
        let encoded = encode_response(protocol, &decoded);
        assert!(
            encoded.to_string().contains("call_weather"),
            "{protocol:?}: {encoded}"
        );
        let events = vec![
            StreamEvent::ToolCallStart {
                index: 0,
                id: "call_weather".into(),
                name: "weather".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: "{\"city\":".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: "\"Paris\"}".into(),
            },
            StreamEvent::ToolCallEnd { index: 0 },
            StreamEvent::Stop(sepia_gateway::protocol::FinishReason::ToolUse),
        ];
        let wire = encode_sse(protocol, &events);
        let decoded = decode_sse(protocol, &wire).unwrap();
        assert!(decoded.iter().any(|event| matches!(event, StreamEvent::ToolCallStart { id, name, .. } if id == "call_weather" && name == "weather")), "{protocol:?}: {wire}");
        assert!(decoded.iter().any(|event| matches!(event, StreamEvent::ToolCallDelta { arguments, .. } if arguments.contains("Paris"))), "{protocol:?}: {wire}");
        assert!(
            decoded.contains(&StreamEvent::Stop(
                sepia_gateway::protocol::FinishReason::ToolUse
            )),
            "{protocol:?}: {wire}"
        );
    }
}

#[test]
fn files_and_audio_are_not_silently_discarded() {
    let input = json!({"model":"alias","messages":[{"role":"user","content":[
        {"type":"input_audio","input_audio":{"data":"aGVsbG8=","format":"wav"}},
        {"type":"file","file":{"file_data":"aGVsbG8=","filename":"notes.txt"}}
    ]}],"max_tokens":64});
    let request = decode_request(Protocol::OpenAI, input, Some("alias")).unwrap();
    assert!(matches!(
        request.messages[0].content[0],
        ContentBlock::Audio { .. }
    ));
    assert!(matches!(
        request.messages[0].content[1],
        ContentBlock::File { .. }
    ));
    let encoded = encode_request(Protocol::Gemini, &request, "upstream").unwrap();
    assert_eq!(
        encoded["contents"][0]["parts"][0]["inlineData"]["mimeType"],
        "audio/wav"
    );
    assert!(encoded.to_string().contains("notes.txt"));
    let _: Value = encoded;
}
