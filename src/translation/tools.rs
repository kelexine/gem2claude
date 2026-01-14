// Tool translation and schema sanitization
// Author: kelexine (https://github.com/kelexine)

use crate::error::Result;
use crate::models::anthropic::Tool as AnthropicTool;
use crate::models::gemini::{
    FunctionCall, FunctionDeclaration, FunctionResponse, Part as GeminiPart, ToolDeclaration,
};
use serde_json::Value;
use tracing::{info};

/// Translate Anthropic tools to Gemini function declarations
pub fn translate_tools(tools: Vec<AnthropicTool>) -> Vec<ToolDeclaration> {

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

    FunctionDeclaration {
        name: tool.name,
        description: tool.description.unwrap_or_default(),
        parameters_json_schema: sanitized_params,
    }
}

/// Sanitize JSON schema for Gemini internal API
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
fn sanitize_additional_properties(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            // Check additionalProperties field
            if let Some(additional) = map.get("additionalProperties") {
                if let Some(obj) = additional.as_object() {
                    if obj.is_empty() {
                        // Empty object {} -> convert to false
                        map.insert("additionalProperties".to_string(), Value::Bool(false));
                    } else if obj.len() == 1 && obj.contains_key("type") {
                        // Keep simple type constraints as-is
                    } else {
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
/// Uses stored thoughtSignature if available, falls back to skip_thought_signature_validator
pub fn translate_tool_use(id: String, name: String, input: Value) -> GeminiPart {
    use crate::translation::signature_store::get_signature;
    use tracing::{debug, info};
    
    // Try to retrieve the original thoughtSignature that Gemini sent with this function call
    let thought_signature = match get_signature(&id) {
        Some(sig) => {
            info!("Using stored thoughtSignature for tool_use_id: {} (sig length: {})", id, sig.len());
            Some(sig)
        }
        None => {
            // Fallback for function calls not generated by Gemini (e.g., from migration)
            info!("No stored signature for tool_use_id: {}, using skip_thought_signature_validator fallback", id);
            Some("skip_thought_signature_validator".to_string())
        }
    };
    
    debug!("Translating tool_use {} -> FunctionCall {} with signature present: {}", 
        id, name, thought_signature.is_some());
    
    GeminiPart::FunctionCall {
        function_call: FunctionCall { name, args: input },
        thought_signature,
    }
}



/// Translate tool result (Anthropic → Gemini FunctionResponse)
pub fn translate_tool_result(
    tool_use_id: String,
    tool_name: String,
    content: String,
    is_error: Option<bool>,
) -> Result<GeminiPart> {
    // Match Gemini CLI's exact format from coreToolScheduler.ts
    let response = if is_error.unwrap_or(false) {
        serde_json::json!({
            "error": content
        })
    } else {
        serde_json::json!({
            "output": content
        })
    };

    Ok(GeminiPart::FunctionResponse {
        function_response: FunctionResponse {
            name: tool_name,
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
