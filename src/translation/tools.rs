// Tool translation and schema sanitization
// Author: kelexine (https://github.com/kelexine)

use crate::error::Result;
use crate::models::anthropic::Tool as AnthropicTool;
use crate::models::gemini::{
    FunctionCall, FunctionDeclaration, FunctionResponse, Part as GeminiPart, ToolDeclaration,
};
use serde_json::Value;
use tracing::{debug, info};

/// Translate Anthropic tools to Gemini function declarations
/// Returns empty vec if no tools provided (important: protobuf requires valid tool_type)
pub fn translate_tools(tools: Vec<AnthropicTool>) -> Vec<ToolDeclaration> {
    // Log incoming tools for debugging complex schema issues
    info!(
        "=== INCOMING TOOLS ({} total) ===",
        tools.len()
    );
    for (i, tool) in tools.iter().enumerate() {
        if let Ok(schema_json) = serde_json::to_string_pretty(&tool.input_schema) {
            info!(
                "Tool[{}] '{}': schema =\n{}",
                i, tool.name, schema_json
            );
        }
    }
    info!("=== END INCOMING TOOLS ===");

    // CRITICAL: Don't create empty ToolDeclaration - protobuf requires valid tool_type
    if tools.is_empty() {
        info!("No tools provided, returning empty vec");
        return vec![];
    }

    vec![ToolDeclaration {
        function_declarations: tools.into_iter().map(translate_tool).collect(),
    }]
}

/// Translate single tool
fn translate_tool(tool: AnthropicTool) -> FunctionDeclaration {
    let sanitized_params = sanitize_schema(tool.input_schema);

    // Log sanitized schema at debug level for verification
    if let Ok(sanitized_json) = serde_json::to_string_pretty(&sanitized_params) {
        debug!(
            "Tool '{}' AFTER sanitization:\n{}",
            tool.name, sanitized_json
        );
    }

    FunctionDeclaration {
        name: tool.name,
        description: tool.description,
        parameters_json_schema: sanitized_params,
    }
}

/// Sanitize JSON schema for Gemini internal API
///
/// The internal API rejects standard JSON Schema keywords like:
/// - $schema, $id, $ref, definitions, $defs
/// - exclusiveMinimum, exclusiveMaximum
/// - propertyNames, patternProperties, additionalItems
/// - format (except 'enum' and 'date-time' for strings)
/// - Numeric constraints: minimum, maximum, minLength, maxLength, minItems, maxItems
/// - default values (can cause issues)
pub fn sanitize_schema(mut schema: Value) -> Value {
    const FORBIDDEN: &[&str] = &[
        // JSON Schema meta keywords
        "$schema",
        "$id",
        "$ref",
        "definitions",
        "$defs",
        // Range constraints not supported
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minimum",
        "maximum",
        // String constraints not supported
        "minLength",
        "maxLength",
        // Array constraints not supported
        "minItems",
        "maxItems",
        // Additional schema keywords not supported
        "propertyNames",
        "patternProperties",
        "additionalItems",
        "default",
        // Pattern not supported
        "pattern",
        // Content keywords not supported
        "contentMediaType",
        "contentEncoding",
    ];

    schema = remove_keys(schema, FORBIDDEN);
    schema = sanitize_format_field(schema);
    schema = sanitize_additional_properties(schema);
    schema = ensure_type_fields(schema);
    schema
}

/// Recursively remove forbidden keys from JSON value
/// Only removes schema-level keywords, NOT property names inside "properties" objects
fn remove_keys(value: Value, forbidden: &[&str]) -> Value {
    remove_keys_impl(value, forbidden, false)
}

fn remove_keys_impl(value: Value, forbidden: &[&str], inside_properties: bool) -> Value {
    match value {
        Value::Object(mut map) => {
            // Determine if we're entering a "properties" block
            let is_properties_block = inside_properties;
            
            // Remove forbidden keys ONLY if we're not inside a "properties" block
            if !is_properties_block {
                map.retain(|k, _| !forbidden.contains(&k.as_str()));
            }
            
            // Recursively clean nested objects
            for (key, v) in map.iter_mut() {
                // Check if this key is "properties" to track context
                let entering_properties = key == "properties" || key == "items";
                *v = remove_keys_impl(v.clone(), forbidden, entering_properties);
            }

            Value::Object(map)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| remove_keys_impl(v, forbidden, inside_properties))
                .collect(),
        ),
        other => other,
    }
}

/// Sanitize format field - only 'enum' and 'date-time' are allowed
fn sanitize_format_field(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            // Check format field
            if let Some(format) = map.get("format") {
                if let Some(format_str) = format.as_str() {
                    if format_str != "enum" && format_str != "date-time" {
                        debug!("Removing unsupported format value: {}", format_str);
                        map.remove("format");
                    }
                }
            }

            // Recursively sanitize nested objects
            for (_, v) in map.iter_mut() {
                *v = sanitize_format_field(v.clone());
            }

            Value::Object(map)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(sanitize_format_field)
                .collect(),
        ),
        other => other,
    }
}

/// Sanitize additionalProperties field
/// - Empty objects {} become false (disallow extra properties)
/// - Complex schema objects are simplified to just type constraints
fn sanitize_additional_properties(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            // Check additionalProperties field
            if let Some(additional) = map.get("additionalProperties") {
                if let Some(obj) = additional.as_object() {
                    if obj.is_empty() {
                        // Empty object {} -> convert to false
                        debug!("Converting empty additionalProperties to false");
                        map.insert("additionalProperties".to_string(), Value::Bool(false));
                    } else if obj.len() == 1 && obj.contains_key("type") {
                        // Keep simple type constraints as-is
                    } else {
                        // Complex schema in additionalProperties - simplify
                        debug!("Simplifying complex additionalProperties schema");
                        map.insert("additionalProperties".to_string(), Value::Bool(true));
                    }
                }
            }

            // Recursively sanitize nested objects
            for (_, v) in map.iter_mut() {
                *v = sanitize_additional_properties(v.clone());
            }

            Value::Object(map)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(sanitize_additional_properties)
                .collect(),
        ),
        other => other,
    }
}

/// Ensure type fields exist where required
fn ensure_type_fields(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            // If no type field and no anyOf/allOf/oneOf, default to "object"
            if !map.contains_key("type")
                && !map.contains_key("anyOf")
                && !map.contains_key("allOf")
                && !map.contains_key("oneOf")
            {
                // If it has properties, it's clearly an object
                if map.contains_key("properties") {
                    map.insert("type".to_string(), serde_json::json!("object"));
                }
            }

            // Recursively ensure types in nested objects
            for (_, v) in map.iter_mut() {
                *v = ensure_type_fields(v.clone());
            }

            Value::Object(map)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(ensure_type_fields)
                .collect(),
        ),
        other => other,
    }
}

/// Translate tool use content block (Anthropic → Gemini)
pub fn translate_tool_use(_id: String, name: String, input: Value) -> GeminiPart {
    GeminiPart::FunctionCall {
        function_call: FunctionCall { name, args: input },
        // Gemini 3 requires thought_signature for function calls in conversation history
        // Use magic value to skip validation
        thought_signature: Some("skip_thought_signature_validator".to_string()),
    }
}

/// Translate tool result content block (Anthropic → Gemini)
pub fn translate_tool_result(
    tool_use_id: String,
    content: String,
    is_error: Option<bool>,
) -> Result<GeminiPart> {
    // We need to track the function name from the previous tool use
    // For now, we'll extract it from the content or use a placeholder
    // In a real implementation, we'd maintain state to track tool calls

    let response = if is_error.unwrap_or(false) {
        serde_json::json!({
            "error": content
        })
    } else {
        serde_json::json!({
            "result": content
        })
    };

    Ok(GeminiPart::FunctionResponse {
        function_response: FunctionResponse {
            name: format!("tool_{}", tool_use_id), // Placeholder - need to track actual name
            response,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_schema_sanitization() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "exclusiveMinimum": 0,
            "$ref": "#/definitions/foo"
        });

        let sanitized = sanitize_schema(schema);

        assert!(!sanitized.get("$schema").is_some());
        assert!(!sanitized.get("exclusiveMinimum").is_some());
        assert!(!sanitized.get("$ref").is_some());
        assert!(sanitized.get("type").is_some());
        assert!(sanitized.get("properties").is_some());
    }

    #[test]
    fn test_nested_schema_sanitization() {
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {
                    "$schema": "should be removed",
                    "type": "string"
                }
            }
        });

        let sanitized = sanitize_schema(schema);
        let nested = sanitized.get("properties").unwrap().get("nested").unwrap();

        assert!(!nested.get("$schema").is_some());
        assert!(nested.get("type").is_some());
    }
}
