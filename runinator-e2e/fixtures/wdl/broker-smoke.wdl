workflow "Brokered Result Path Smoke" v1 {
    // single console action; the e2e test asserts on the `write_logs` node run and its streamed
    // stdout chunks, so the two markers must reach the broker result path.
    node write_logs <- Console.run(command: "echo broker-smoke-start; echo broker-smoke-end").timeout(10s)
}
