use runinator_wdl::{CompileOptions, compile_str};

fn try_compile(label: &str, src: &str) {
    match compile_str(src, &CompileOptions::default()) {
        Ok(def) => println!("OK   [{label}] input_type = {:?}", def.input_type),
        Err(e) => println!("ERR  [{label}] {e}"),
    }
}

#[test]
fn probe() {
    try_compile(
        "params open w/ field, typed tail",
        r#"
        workflow "A" v1 { params { name: string ...: integer } node console.run(command: params.name) }
    "#,
    );
    try_compile(
        "params open no fields, typed tail",
        r#"
        workflow "A" v1 { params { ...: any } node console.run(command: "x") }
    "#,
    );
    try_compile(
        "params bare ellipsis (no type)",
        r#"
        workflow "A" v1 { params { name: string ... } node console.run(command: params.name) }
    "#,
    );
    try_compile(
        "params only bare ellipsis",
        r#"
        workflow "A" v1 { params { ... } node console.run(command: "x") }
    "#,
    );
    try_compile(
        "type decl typed tail",
        r#"
        workflow "A" v1 { type T { id: string ...: any } params { p: T } node console.run(command: "x") }
    "#,
    );
    try_compile(
        "type decl bare ellipsis",
        r#"
        workflow "A" v1 { type T { id: string ... } params { p: T } node console.run(command: "x") }
    "#,
    );
}
