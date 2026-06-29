mod app;
mod client;
mod commands;
mod discovery;
mod error;
mod pack_dev;
mod state;
mod types;
mod worker;

fn main() {
    app::run();
}
