mod app;
mod client;
mod commands;
mod discovery;
mod error;
mod pack_dev;
mod service;
mod state;
mod types;

fn main() {
    service::CommandCenterService::new().run();
}
