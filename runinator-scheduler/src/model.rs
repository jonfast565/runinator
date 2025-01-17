use std::pin::Pin;

pub struct SchedulerTaskFuture {
    pub future: Pin<Box<tokio::task::JoinHandle<()>>>,
}

impl SchedulerTaskFuture {
    pub fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        SchedulerTaskFuture {
            future: Box::pin(handle),
        }
    }
}