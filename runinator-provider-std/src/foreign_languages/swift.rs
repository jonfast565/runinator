use super::ForeignLanguageAdapter;

pub(super) static SWIFT: Swift = Swift;

pub(super) struct Swift;

impl ForeignLanguageAdapter for Swift {
    fn canonical(&self) -> &'static str {
        "swift"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.swift"
    }

    fn runner_filename(&self) -> &'static str {
        "main.swift"
    }

    fn runner_source(&self) -> &'static str {
        r#"import Foundation

let environment = ProcessInfo.processInfo.environment
let contextData = try Data(contentsOf: URL(fileURLWithPath: environment["RUNINATOR_CONTEXT"]!))
let context = try JSONSerialization.jsonObject(with: contextData, options: [.fragmentsAllowed])
let result = try main(context)
let outputData = try JSONSerialization.data(withJSONObject: result, options: [.fragmentsAllowed])
try outputData.write(to: URL(fileURLWithPath: environment["RUNINATOR_OUTPUT"]!))
"#
    }

    fn execute(&self) -> &'static str {
        "swiftc -module-cache-path /tmp/runinator-module-cache /work/foreign.swift /work/main.swift -o /tmp/runinator_foreign && /tmp/runinator_foreign"
    }
}
