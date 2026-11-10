use sepia_gateway::protocol::{FinishReason, Protocol, StreamEvent, decode_sse};

#[test]
fn captured_anthropic_terminal_sequence_emits_stop() {
    let input = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    let events = decode_sse(Protocol::Anthropic, input).unwrap();
    assert!(
        events.contains(&StreamEvent::Stop(FinishReason::Stop)),
        "{events:?}"
    );
}
