workflow "Hello World Test" v1 {
    // run a single console command, then terminate at the synthetic end node.
    let greeting = console.run(command: "echo hello world")
        -> done
}
