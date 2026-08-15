use super::*;

#[test]
fn openapi_operations_become_mcp_tools() {
    let document = json!({
        "paths": {
            "/widgets/{id}": {
                "get": {
                    "summary": "Fetch a widget",
                    "description": "Returns one widget by identifier.",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
                        { "name": "verbose", "in": "query", "required": false, "schema": { "type": "boolean" } }
                    ],
                    "responses": { "200": {} }
                }
            }
        }
    });

    let tools = api_tools_from_openapi(&document);

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "runinator_api_get_widgets_id");
    assert_eq!(tools[0].method, "get");
    assert_eq!(tools[0].path, "/widgets/{id}");
    assert_eq!(
        tools[0].definition()["inputSchema"]["required"],
        json!(["id"])
    );
    assert_eq!(
        tools[0].definition()["inputSchema"]["properties"]["verbose"]["type"],
        "boolean"
    );
    assert_eq!(tools[0].definition()["annotations"]["readOnlyHint"], true);
}

#[test]
fn request_schemas_carry_only_their_referenced_definitions() {
    let document = json!({
        "components": {
            "schemas": {
                "CreateWidget": {
                    "type": "object",
                    "properties": { "owner": { "$ref": "#/components/schemas/Owner" } }
                },
                "Owner": { "type": "object", "properties": { "name": { "type": "string" } } },
                "Unrelated": { "type": "object" }
            }
        },
        "paths": {
            "/widgets": {
                "post": {
                    "summary": "Create a widget",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateWidget" } } }
                    },
                    "responses": { "200": {} }
                }
            }
        }
    });

    let tools = api_tools_from_openapi(&document);
    let schema = &tools[0].definition()["inputSchema"];

    assert_eq!(schema["properties"]["body"]["$ref"], "#/$defs/CreateWidget");
    assert!(schema["$defs"]["CreateWidget"].is_object());
    assert!(schema["$defs"]["Owner"].is_object());
    assert!(schema["$defs"].get("Unrelated").is_none());
    assert_eq!(schema["required"], json!(["body"]));
}

#[test]
fn websocket_upgrade_operations_are_not_advertised() {
    let document = json!({
        "paths": {
            "/ws": {
                "get": {
                    "summary": "Open a websocket",
                    "responses": { "101": { "description": "switching protocols" } }
                }
            }
        }
    });

    assert!(api_tools_from_openapi(&document).is_empty());
}
