#[allow(unused_imports)]
use super::*;

pub(super) struct Server<'a> {
    pub(super) client: &'a Client,
    pub(super) api_base_url: &'a str,
    pub(super) options: Options,
    pub(super) capture: &'a mut OutputCapture,
}

impl Server<'_> {
    /// the frame to write back, or nothing when the line was a notification.
    pub(super) async fn respond(&mut self, line: &str) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(broken) => return Some(failure(Value::Null, PARSE_ERROR, broken.to_string())),
        };
        // a notification carries no id and takes no reply — `notifications/initialized` is the one
        // every client sends, and answering it is a protocol error.
        let id = request.get("id").cloned()?;
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        Some(self.dispatch(id, method, params).await)
    }

    pub(super) async fn dispatch(&mut self, id: Value, method: &str, params: Value) -> Value {
        match method {
            "initialize" => success(id, self.initialize()),
            "ping" => success(id, json!({})),
            "tools/list" => success(id, json!({ "tools": self.tool_definitions().await })),
            "tools/call" => self.call(id, params).await,
            "resources/list" if self.options.mission_only => {
                success(id, json!({ "resources": [] }))
            }
            "resources/list" => success(
                id,
                json!({ "resources": resources::list(self.client).await }),
            ),
            "resources/templates/list" if self.options.mission_only => {
                success(id, json!({ "resourceTemplates": [] }))
            }
            "resources/templates/list" => {
                success(id, json!({ "resourceTemplates": resources::templates() }))
            }
            "resources/read" if self.options.mission_only => internal_error(
                id,
                "the mission MCP profile does not expose general resources",
            ),
            "resources/read" => match params.get("uri").and_then(Value::as_str) {
                Some(uri) => match resources::read(self.client, uri).await {
                    Ok(contents) => success(id, contents),
                    Err(message) => internal_error(id, message),
                },
                None => internal_error(id, "resources/read needs a 'uri'"),
            },
            method => failure(
                id,
                METHOD_NOT_FOUND,
                format!("this server does not implement '{method}'"),
            ),
        }
    }

    pub(super) fn initialize(&self) -> Value {
        json!({
            "protocolVersion": protocol::PROTOCOL_VERSION,
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
            },
            "instructions": tools::INSTRUCTIONS,
        })
    }

    /// the tools, which are the command surface plus whatever else is switched on.
    ///
    /// the workflow tools are the only ones that need the web service to list, so an unreachable
    /// server still advertises the command line — which is what the caller needs to find out *why*
    /// it is unreachable.
    pub(super) async fn tool_definitions(&self) -> Vec<Value> {
        if self.options.mission_only {
            return tools::mission_definitions();
        }
        let workflows = match self.options.workflow_tools {
            true => self.client.fetch_workflows().await.unwrap_or_default(),
            false => Vec::new(),
        };
        tools::definitions(workflow_tools::definitions(workflows))
    }

    pub(super) async fn call(&mut self, id: Value, params: Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return internal_error(id, "tools/call needs a 'name'");
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        // a tool that failed still answers with a result carrying `isError`; only a call that could
        // not be read at all is a transport error.
        success(id, self.tool(name, arguments).await)
    }

    pub(super) async fn tool(&mut self, name: &str, arguments: Value) -> Value {
        if self.options.mission_only {
            let Some(tool) = schema::find_mission(name) else {
                return protocol::text_result(
                    format!("'{name}' is not available to a harnessed mission session"),
                    true,
                );
            };
            return self.command_tool(tool, &arguments).await;
        }
        if name == tools::HELP_TOOL {
            return tools::help(&arguments);
        }
        if name == tools::EXEC_TOOL {
            return self.exec_tool(&arguments).await;
        }
        if let Some(tool) = schema::find(name) {
            return self.command_tool(tool, &arguments).await;
        }
        if self.options.workflow_tools && workflow_tools::workflow_id_for(name).is_some() {
            return workflow_tools::call(self.client, name, arguments).await;
        }
        protocol::text_result(
            format!("no tool named '{name}'. call tools/list to see what there is."),
            true,
        )
    }

    pub(super) async fn exec_tool(&mut self, arguments: &Value) -> Value {
        let command = match protocol::required_str(arguments, "command") {
            Ok(command) => command,
            Err(message) => return protocol::text_result(message, true),
        };
        // json is the default: a model reads a payload better than a table, and every command takes
        // the flag even when it prints the same either way.
        let json = arguments
            .get("json")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let timeout = arguments
            .get("timeout_seconds")
            .and_then(Value::as_i64)
            .filter(|seconds| *seconds > 0)
            .map(|seconds| Duration::from_secs(seconds as u64))
            .unwrap_or(self.options.timeout);
        exec::exec(
            self.client,
            self.capture,
            &command,
            json,
            timeout,
            self.api_base_url,
        )
        .await
    }

    pub(super) async fn command_tool(
        &mut self,
        tool: &schema::CommandTool,
        arguments: &Value,
    ) -> Value {
        let mission_id = if self.options.mission_only {
            self.options.mission_id
        } else {
            None
        };
        let arguments = fence_mission_arguments(arguments, mission_id);
        let line = match schema::command_line(tool, &arguments) {
            Ok(line) => line,
            // a rejected argument is the model's to read and fix, so it comes back as a tool error
            // with the command's own argument names in it rather than as a transport failure.
            Err(message) => return protocol::text_result(message, true),
        };
        exec::run(
            self.client,
            self.capture,
            line,
            true,
            self.options.timeout,
            self.api_base_url,
            &tool.path.join(" "),
        )
        .await
    }
}
