#[no_mangle]
pub extern "C" fn example_action(task_name: &str) {
    println!("Executing action for task: {}", task_name);
}

#[no_mangle]
pub extern "C" fn is_runinator_plugin() {}