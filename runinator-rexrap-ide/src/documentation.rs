//! Short, editor-facing explanations of REXRAP's fixed language surface.
//!
//! Keep this catalog exhaustive. `lib_tests` checks it against the grammar-generated keyword
//! vocabulary so a grammar addition cannot degrade to a generic editor tooltip.

/// Returns documentation for a fixed REXRAP word in any of its language positions.
pub(crate) fn keyword_documentation(keyword: &str) -> Option<&'static str> {
    Some(match keyword {
        "_" => "A placeholder name for a value that is intentionally not used.",
        "action" => "Names the action recorded by an `audit` statement.",
        "active" => "Matches an ingress event while its correlation has a live run.",
        "actor" => "Sets the actor recorded by an `audit` statement.",
        "after" => "Sets the duration threshold before a notification event fires.",
        "alias" => {
            "Declares a reusable object of action arguments that can be spread with `...name`."
        }
        "all" => {
            "Selects every matching branch, item, or awaited run, depending on the enclosing policy."
        }
        "allow" => "Allows a new workflow run when its concurrency limit has been reached.",
        "allow_self_originated" => {
            "Allows an orchestration intent to react to an event it originated itself."
        }
        "any" => "The permissive type for a value whose shape is not known statically.",
        "app" => "Sends a notification through the in-app notification channel.",
        "approve" => "Parks the workflow until a person approves or rejects the request.",
        "as" => "Introduces an import alias or casts an expression to a type.",
        "assert" => "Checks named invariants and fails the step when one condition is false.",
        "async" => {
            "Schedules a call as a task instead of joining it inline; asyncness belongs to the call site."
        }
        "attempts" => {
            "Sets the maximum number of attempts allowed by an orchestration failure budget."
        }
        "audit" => "Records structured compliance information for the current workflow run.",
        "await" => "Joins a `task[T]` handle or waits for runs of another workflow.",
        "barrier" => "Waits until a named cross-run rendezvous reaches its required count.",
        "blackout" => "Excludes a time range or schedule from a trigger's eligible firing times.",
        "branch" => "Introduces one body of a `parallel` or `race` control-flow region.",
        "budget" => {
            "Sets an attempt limit and exhaustion behavior for an orchestration failure class."
        }
        "cancel" => {
            "Cancels a workflow run or selects cancellation as an orchestration stop policy."
        }
        "cancel_previous" => {
            "Cancels an existing run before admitting a new run at the concurrency limit."
        }
        "catch" => "Runs a recovery block when the preceding `try` block fails.",
        "catchup" => "Controls what a trigger does with slots missed while it was inactive.",
        "checkpoint" => "Stores a named snapshot of the current workflow state.",
        "child" => "An interrupt source that fires when a subflow child reaches a terminal state.",
        "circuit_breaker" => "Guards work with a cross-run failure threshold and cooldown window.",
        "coalesce" => {
            "Accumulates matching orchestration intents until the configured durable wake deadline."
        }
        "collect" => "Accumulates values under a named, bounded collection window.",
        "compensate" => {
            "Attaches a saga rollback action that runs if later work drives the run to failure."
        }
        "complete" => {
            "Selects a completed workflow or pipeline as a chained-trigger or link outcome."
        }
        "compute" => "Runs a pure in-process computation block; it never schedules provider work.",
        "concurrency" => "Limits simultaneous work in a workflow, map, or pipeline context.",
        "condition" => "Selects a condition-driven gate or policy mode.",
        "config" => "Declares or references a non-secret configuration value.",
        "contains" => "Tests whether a collection or string contains a value.",
        "continue" => "Routes control to a named continuation or terminal target.",
        "cooldown" => "Limits a named operation to one successful pass per duration window.",
        "correlate" => {
            "Declares the key by which this workflow run can be awaited or addressed by ingress."
        }
        "correlations" => {
            "Maps a pipeline member result field into durable orchestration correlation state."
        }
        "count" => "Sets the number of items or arrivals required by a collection or barrier.",
        "critical" => "Sets the highest notification severity.",
        "cron" => "Schedules a workflow or pipeline from a cron expression.",
        "current" => "Restarts orchestration processing from the current member or epoch position.",
        "d" => "The days suffix in a duration literal, such as `2d`.",
        "debounce" => "Delays work until a named key has stayed quiet for the configured duration.",
        "delay" => "Sets the trailing delay for a `debounce` statement.",
        "description" => "Supplies human-readable text for a pipeline declaration.",
        "detach" => "Drops a task handle without joining it.",
        "dir" => "Includes file paths from a directory in a pure expression.",
        "disabled" => {
            "Keeps a declared trigger, interrupt, or notification inactive without removing it."
        }
        "dispatch" => "Routes a matching ingress event to a named orchestration intent.",
        "do" => "Introduces the runtime block containing the statements a workflow executes.",
        "effect" => "Selects the generic control effect produced by an orchestration intent.",
        "else" => "Introduces the fallback branch of an `if`, `match`, or `split` region.",
        "email" => "Sends a notification through the email channel.",
        "emit" => "Emits workflow output data or an event without declaring artifacts.",
        "end" => "A successful terminal route target.",
        "ends_with" => "Tests whether a string ends with the supplied value.",
        "entry" => "Names the pipeline member or restart point used as an entry.",
        "enum" => "Introduces a closed type whose value must be one of the listed literals.",
        "event_source" => {
            "Iterates over events of a declared type with optional filtering and bounds."
        }
        "every" => "Sets the polling interval for a wait, gate, lock, or interrupt source.",
        "evidence" => "Maps a pipeline member result field into durable orchestration evidence.",
        "exhausted" => "Selects the behavior after an orchestration failure budget is consumed.",
        "exists" => "Tests whether a referenced value is present.",
        "external" => {
            "Selects an externally resolved gate or an externally delivered interrupt source."
        }
        "fail" => "Fails the current step or selects a failed terminal route target.",
        "failure" => "Selects a failure outcome, notification event, or interrupt source.",
        "failure_class" => "Maps or names the failure category used by orchestration budgets.",
        "false" => "The boolean false literal.",
        "file" => {
            "Creates a file reference from a path in an expression; as a type, represents a file value."
        }
        "filter" => "Applies a condition that selects which events an `event_source` accepts.",
        "finally" => "Runs a cleanup block after `try`, whether the body succeeded or failed.",
        "fire_all" => "Replays each missed trigger slot, subject to its configured maximum.",
        "fire_once" => "Collapses missed trigger slots into one catch-up run.",
        "first_success" => "Selects the first successful branch or upstream pipeline member.",
        "fn" => "Declares a reusable pure function, or pairs with `task` to declare runtime work.",
        "for" => "Iterates a body over a collection, optionally with a typed item and limit.",
        "from" => "Maps an orchestration field from a result pointer.",
        "function" => "Introduces a first-class function type, written `function<(T) -> R>`.",
        "functions" => "Selects function exports as the kind of an import.",
        "gate" => "Parks the workflow behind a manual, condition, or external gate.",
        "goto" => "Transfers control from a compute block to a named target.",
        "grace" => "Sets how late a missed trigger slot may be before catch-up skips it.",
        "h" => "The hours suffix in a duration literal, such as `6h`.",
        "halt" => "Stops a pipeline after a member failure under the pipeline failure policy.",
        "hold" => "Sets how long a mutex lease is retained after it is acquired.",
        "if" => "Runs a branch only when its condition is true.",
        "import" => "Opens a workflow, function, settings, or module namespace in local scope.",
        "in" => "Tests membership or introduces the collection iterated by `for` and `map`.",
        "info" => "Sets the lowest notification severity.",
        "ingress" => "Declares a correlation scope and lifecycle-aware event routes.",
        "initial" => "Sets the initial time or state used by a wait operation.",
        "inline" => "Embeds a raw code block as a string value in a pure expression.",
        "input" => "Parks the workflow until an input value is supplied.",
        "inquire" => "Requests human intervention when a pipeline member fails.",
        "intent" => {
            "Maps an author-defined name and priority to a generic orchestration control effect."
        }
        "interrupt" => "Declares a handler region that suspends the run when its source fires.",
        "join" => "Declares a named continuation or a pipeline fan-in policy.",
        "json" => {
            "Converts a value to JSON text; as a type alias, accepts an unconstrained JSON value."
        }
        "key" => {
            "Sets a stable workflow key, correlation key, or scope key for the enclosing construct."
        }
        "labels" => "Sets labels on a workspace policy.",
        "language" => "Declares the REXRAP language version at the start of a source document.",
        "lease" => "Sets the duration of a durable workspace lease.",
        "let" => "Binds a statement or compute result to a local name.",
        "limit" => "Caps the number of loop iterations or items processed.",
        "m" => "The minutes suffix in a duration literal, such as `15m`.",
        "manual" => "Selects a gate that only a person or external control can resolve.",
        "map" => "Iterates a body over a collection with configurable concurrency.",
        "match" => "Selects a branch by matching values or predicates.",
        "max" => "Sets the maximum number of replayed slots or collected items.",
        "max_depth" => "Caps recursive function calls or pipeline traversal depth.",
        "max_epochs" => "Caps the number of correlated orchestration epochs that may run.",
        "meta" => "Sets metadata on an input request or configuration statement.",
        "mode" => "Selects the waiting or aggregation behavior of the enclosing construct.",
        "module" => "Declares a pack-local source module or selects a module import kind.",
        "mutex" => "Acquires, releases, or scopes a cross-run exclusive lock.",
        "name" => "Sets a name field in the enclosing configuration or declaration.",
        "namespace" => "Sets the namespace that qualifies a workflow or source module.",
        "next" => "Selects the normal next edge when resuming an interrupt handler.",
        "next_member" => "Maps the next pipeline member into durable orchestration state.",
        "none" => "Disables an optional limit, stop action, or concurrency cap.",
        "notify" => "Declares an import-managed notification policy for workflow events.",
        "null" => "The null literal and the type for an explicitly absent value.",
        "observe" => "Selects an orchestration effect that records or observes current state.",
        "off" => "Introduces the false branch of a `toggle` statement.",
        "on" => "Introduces an event, outcome, source, or selector clause.",
        "on_complete" => {
            "Chains work when a source workflow or pipeline reaches any terminal state."
        }
        "on_conflict" => {
            "Selects what a trigger does when the workflow concurrency cap is reached."
        }
        "on_failure" => "Chains work or chooses policy when a source workflow or pipeline fails.",
        "on_success" => "Chains work when a source workflow or pipeline succeeds.",
        "on_timeout" => "Selects whether an expired gate fails or continues.",
        "orchestration" => {
            "Declares correlated-execution policy: intents, failure budgets, mappings, and workspace requirements."
        }
        "orphan_signal" => {
            "An interrupt source for a signal that arrived while no node was waiting for it."
        }
        "output" => "Declares run-level artifacts and emitted events in an output block.",
        "parallel" => "Runs branches concurrently and joins them according to an optional policy.",
        "params" => "Declares the workflow input schema and default values.",
        "parked" => {
            "Selects the notification event for a run that remains parked past its threshold."
        }
        "pause" => "Pauses a run or selects pausing as an orchestration outcome.",
        "per" => "Sets the duration window used to measure a throttle rate.",
        "phase" => "Configures one pipeline member's result mappings and workspace policy.",
        "pipeline" => "Declares a static graph composed from member workflows.",
        "priority" => "Sets ordering among predicate routes or orchestration intents.",
        "profile" => "Selects an execution profile through a step attribute.",
        "queue" => {
            "Queues work rather than discarding it when an ingress or concurrency policy cannot run it now."
        }
        "race" => "Runs branches concurrently and continues according to a winner policy.",
        "range" => "Constricts a numeric or duration type to inclusive lower and upper bounds.",
        "rate" => "Sets how many operations a throttle permits in each window.",
        "reason" => "Sets the reason recorded by an `audit` statement.",
        "record" => "Records a matching ingress event without starting or interrupting a run.",
        "recovery" => "Selects how a workspace policy recovers an unavailable workspace.",
        "reject" => "Selects the route taken when a human approval is rejected.",
        "release" => "Releases a named mutex.",
        "replace" => "Replaces an unavailable workspace or selects replacement recovery behavior.",
        "requeue" => "Places a matching ingress event back into its queue.",
        "resolved" => {
            "An interrupt source for a signal, approval, or input that has been resolved."
        }
        "resources" => "Maps a member result field into durable orchestration resources.",
        "resources_patch" => "Maps a partial resource update into durable orchestration state.",
        "restart" => "Restarts the interrupted node or selects an orchestration restart target.",
        "resume" => {
            "Ends an interrupt handler and returns control to the suspended workflow thread."
        }
        "retry" => "An interrupt source that fires before a step is dispatched again.",
        "retry_exhausted" => {
            "Selects the notification event emitted when a step has no retries left."
        }
        "return" => {
            "Returns a value from a function or finishes a workflow successfully with a result."
        }
        "returns" => "Declares the result type a workflow exposes to callers.",
        "reuse" => "Reuses a compatible durable workspace for the configured scope.",
        "revision" => {
            "Selects a referenced import revision or maps a subject revision for stale-result rejection."
        }
        "routes" => "Attaches explicit outgoing edges to the preceding statement.",
        "s" => "The seconds suffix in a duration literal, such as `30s`.",
        "schedule" => "Declares a structured schedule trigger or blackout window.",
        "scope" => "Names the correlation or workspace scope used by the enclosing declaration.",
        "secret" => "Declares or references a secret value available only at run time.",
        "set" => "Sets a configuration name or metadata value.",
        "settings" => "Selects settings as the kind of an import.",
        "severity" => "Sets the severity assigned to a notification policy.",
        "signal" => "Waits for an external signal or selects signal as an orchestration effect.",
        "silently_continue" => {
            "Lets a pipeline continue after a member failure without surfacing it as a blocking failure."
        }
        "skip" => "Drops a trigger firing or selects a no-run concurrency policy.",
        "sla" => "Selects the notification event for a run exceeding its service-level threshold.",
        "slack" => "Sends a notification through the Slack channel.",
        "split" => "Routes traffic into percentage-based branches using a stable expression value.",
        "start" => "Declares an explicit workflow entry target or starts a run from ingress.",
        "starts_with" => "Tests whether a string begins with the supplied value.",
        "stop" => "Selects what an orchestration does to the previous epoch before moving on.",
        "string" => "Converts a value to text when called; as a type, represents UTF-8 text.",
        "subflow" => "Runs another workflow as a child flow, optionally detached.",
        "subject_revision" => {
            "Maps the provider-neutral subject revision used to reject stale events and results."
        }
        "success" => "Selects the successful outcome of a route, trigger, or pipeline link.",
        "supersede" => {
            "Selects an orchestration effect that replaces an older correlated run or epoch."
        }
        "suspend" => "Selects an orchestration effect that suspends a correlated run or epoch.",
        "target" => "Sets the target recorded by an `audit` statement.",
        "task" => "Marks a runtime function or describes an awaitable `task[T]` call result.",
        "terminal" => {
            "Matches an ingress event after its correlation has reached a terminal run state."
        }
        "terminate" => "Selects an orchestration effect that terminates the correlated work.",
        "threshold" => "Sets the number of failures that opens a circuit breaker.",
        "throttle" => "Limits a named cross-run operation to a configured rate.",
        "timeout" => "Sets a deadline for a node, gate, lock, or aggregation operation.",
        "timer" => "An interrupt source that fires from the workflow timer.",
        "to" => "Separates the start and end of a trigger blackout range.",
        "toggle" => "Selects an `on` or `off` body from a boolean expression.",
        "transform" => "Builds a reshaped data object from named expression bindings.",
        "trigger" => "Declares an import-managed workflow or pipeline trigger.",
        "true" => "The boolean true literal.",
        "try" => "Runs a body with optional `catch` and `finally` recovery branches.",
        "type" => "Declares a reusable named type or labels an approval or event type.",
        "unbound" => "Matches an ingress event whose correlation does not yet have a run.",
        "until" => "Waits or repeats until its condition becomes true.",
        "v" => "The prefix of a workflow version literal, such as `v1` or `v1.2.3`.",
        "via" => "Names the pipeline member through which a failure budget is evaluated.",
        "wait" => "Parks the workflow for a duration or until a condition becomes true.",
        "wake" => "An interrupt source that fires when a wait deadline elapses.",
        "warning" => "Sets the middle notification severity.",
        "watch" => "Declares a workflow-level cancellation guard.",
        "when" => "Introduces a condition or lifecycle predicate for a branch or route.",
        "while" => "Repeats a body while its condition is true.",
        "window" => "Sets the measurement period used by a circuit breaker.",
        "winner" => "Sets the policy that chooses the winning `race` branch.",
        "with" => {
            "Supplies an object of arguments, input, metadata, or link data to the enclosing construct."
        }
        "with_workspace" => "Binds a pipeline member to a workspace expression.",
        "workflow" => {
            "Declares a workflow, selects a workflow import, or names a workflow to start or await."
        }
        "workspace" => {
            "Requests a durable opaque workspace lease for a pipeline phase or workflow declaration."
        }
        "yield" => "Returns a value from a control-flow region.",
        _ => return None,
    })
}

/// Returns documentation for each named type form accepted by a REXRAP type expression.
pub(crate) fn type_documentation(name: &str) -> Option<&'static str> {
    Some(match name {
        "any" => {
            "A permissive value of any shape. Prefer a more specific type when its structure is known."
        }
        "boolean" | "bool" => "A true-or-false value. `bool` is an alias for `boolean`.",
        "duration" => "A span of time, written with a unit such as `30s`, `5m`, `2h`, or `1d`.",
        "file" => "A file reference passed between compatible provider actions.",
        "float" => {
            "A numeric value that may contain a fractional component. `float` is an alias for `number`."
        }
        "function" => {
            "A first-class callable type, written `function<(Parameter, ...) -> Result>`."
        }
        "int" | "integer" => "A whole-number value. `int` is an alias for `integer`.",
        "json" => "An unconstrained JSON value. `json` is an alias for `any`.",
        "map" => "A map with string keys and values of one type, written `map<Value>`.",
        "null" => "The explicit absence-of-a-value type.",
        "number" => "A numeric value, including integer and fractional values.",
        "range" => {
            "A constraint on a numeric or duration type, written `Type range minimum..maximum`."
        }
        "string" => "A UTF-8 text value.",
        "task" => {
            "An awaitable result handle, written `task[Result]`; bare `task` lets the editor infer its result type."
        }
        "enum" => "A closed set of literal values, written `enum[\"one\", \"two\"]`.",
        _ => return None,
    })
}

/// Returns the canonical surface spelling shown beside a type-constructor hover.
pub(crate) fn type_syntax(name: &str) -> Option<&'static str> {
    match name {
        "enum" => Some("enum[\"value\", ...]"),
        "function" => Some("function<(Parameter, ...) -> Result>"),
        "map" => Some("map<Value>"),
        "range" => Some("Type range minimum..maximum"),
        "task" => Some("task[Result]"),
        _ => None,
    }
}

/// Type surface words that completion offers alongside language constructs.
pub(crate) const TYPE_COMPLETION_WORDS: &[&str] = &[
    "any", "boolean", "bool", "duration", "enum", "file", "float", "function", "int", "integer",
    "json", "map", "null", "number", "range", "string", "task",
];
