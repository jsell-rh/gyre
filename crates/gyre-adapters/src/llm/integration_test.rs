//! Integration test for LLM tool-calling.
//!
//! Run with: cargo test -p gyre-adapters llm_tool_calling_integration --ignored
//!
//! Requires GYRE_VERTEX_PROJECT (or VERTEXAI_PROJECT) and ADC credentials.

#[cfg(test)]
mod tests {
    use gyre_ports::{
        ContentBlock, ConversationContent, ConversationMessage, LlmPortFactory, ToolDefinition,
    };
    use serde_json::json;

    fn make_factory() -> Option<crate::llm::rig_vertexai::RigVertexAiFactory> {
        // Try various env var names
        if std::env::var("GYRE_VERTEX_PROJECT").is_err() {
            if let Ok(p) = std::env::var("VERTEXAI_PROJECT") {
                std::env::set_var("GYRE_VERTEX_PROJECT", &p);
            }
        }
        if std::env::var("GYRE_VERTEX_LOCATION").is_err() {
            if let Ok(l) = std::env::var("VERTEXAI_LOCATION") {
                std::env::set_var("GYRE_VERTEX_LOCATION", &l);
            }
        }
        crate::llm::rig_vertexai::RigVertexAiFactory::from_env().ok()
    }

    #[tokio::test]
    #[ignore] // Requires real credentials
    async fn llm_tool_calling_with_simple_tool() {
        let factory = match make_factory() {
            Some(f) => f,
            None => {
                eprintln!("Skipping: GYRE_VERTEX_PROJECT not set");
                return;
            }
        };
        let port = factory.for_model("claude-sonnet-4-6");

        let tools = vec![ToolDefinition {
            name: "get_weather".to_string(),
            description: "Get the current weather for a location.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string", "description": "City name" }
                },
                "required": ["location"]
            }),
        }];

        let messages = vec![ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Text(
                "What's the weather in Paris? Use the get_weather tool.".to_string(),
            ),
        }];

        let response = port
            .complete_with_tools(
                "You are a helpful assistant. Use tools when asked.",
                &messages,
                &tools,
                Some(1024),
            )
            .await
            .expect("Tool-calling completion should succeed");

        // The model should request to use the tool
        assert!(
            !response.tool_calls.is_empty(),
            "Expected tool calls, got none. Text: {}",
            response.text
        );
        assert_eq!(response.tool_calls[0].name, "get_weather");
        assert!(
            response.tool_calls[0]
                .input
                .get("location")
                .and_then(|v| v.as_str())
                .is_some(),
            "Expected location in tool input"
        );
        assert_eq!(response.stop_reason, "tool_use");

        eprintln!(
            "Tool call: {} with input: {}",
            response.tool_calls[0].name,
            serde_json::to_string(&response.tool_calls[0].input).unwrap()
        );
    }

    #[tokio::test]
    #[ignore] // Requires real credentials
    async fn llm_tool_calling_multi_turn() {
        let factory = match make_factory() {
            Some(f) => f,
            None => {
                eprintln!("Skipping: GYRE_VERTEX_PROJECT not set");
                return;
            }
        };
        let port = factory.for_model("claude-sonnet-4-6");

        let tools = vec![ToolDefinition {
            name: "graph_summary".to_string(),
            description: "Get a summary of the knowledge graph.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }];

        // Turn 1: User asks a question
        let messages = vec![ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Text(
                "Tell me about the codebase. Use the graph_summary tool first.".to_string(),
            ),
        }];

        let response1 = port
            .complete_with_tools(
                "You are the Gyre Explorer agent. Always use tools when available.",
                &messages,
                &tools,
                Some(1024),
            )
            .await
            .expect("Turn 1 should succeed");

        assert!(
            !response1.tool_calls.is_empty(),
            "Expected tool call in turn 1"
        );
        assert_eq!(response1.tool_calls[0].name, "graph_summary");

        // Turn 2: Provide tool result and get final answer
        let mut messages2 = messages.clone();
        messages2.push(ConversationMessage {
            role: "assistant".to_string(),
            content: ConversationContent::Blocks(vec![
                ContentBlock::Text {
                    text: response1.text.clone(),
                },
                ContentBlock::ToolUse {
                    id: response1.tool_calls[0].id.clone(),
                    name: "graph_summary".to_string(),
                    input: json!({}),
                },
            ]),
        });
        messages2.push(ConversationMessage {
            role: "user".to_string(),
            content: ConversationContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: response1.tool_calls[0].id.clone(),
                content: r#"{"node_counts": {"type": 55, "function": 265, "endpoint": 73}, "edge_counts": {"calls": 129, "contains": 684}, "modules": ["domain", "api", "db"]}"#.to_string(),
            }]),
        });

        let response2 = port
            .complete_with_tools(
                "You are the Gyre Explorer agent. Always use tools when available.",
                &messages2,
                &tools,
                Some(1024),
            )
            .await
            .expect("Turn 2 should succeed");

        // The model should respond with text (no more tool calls needed)
        assert!(
            !response2.text.is_empty(),
            "Expected text response in turn 2"
        );
        eprintln!("Turn 2 response: {}", response2.text);
    }
}
