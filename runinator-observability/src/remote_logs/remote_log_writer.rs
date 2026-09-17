#[allow(unused_imports)]
use super::*;

pub struct RemoteLogWriter {
    pub(super) source: String,
    pub(super) buffer: Vec<u8>,
}

impl Write for RemoteLogWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RemoteLogWriter {
    fn drop(&mut self) {
        if EXPORTING.with(Cell::get) {
            return;
        }
        let Some(publisher) = PUBLISHER.get() else {
            return;
        };
        for line in String::from_utf8_lossy(&self.buffer)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let message = line.chars().take(16 * 1024).collect::<String>();
            let size = message.len();
            let previous = publisher.queued_bytes.fetch_add(size, Ordering::Relaxed);
            if previous.saturating_add(size) > MEMORY_LIMIT {
                publisher.queued_bytes.fetch_sub(size, Ordering::Relaxed);
                publisher.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let (level, target) = level_and_target(&message, &self.source);
            let record = RuntimeLogRecord {
                event_id: uuid::Uuid::now_v7(),
                occurred_at: chrono::Utc::now(),
                source: self.source.clone(),
                runtime_id: env::var("RUNINATOR_RUNTIME_ID").ok(),
                replica_id: env::var("RUNINATOR_REPLICA_ID")
                    .ok()
                    .and_then(|value| value.parse().ok()),
                level: level.into(),
                target,
                message,
                dropped_before: 0,
                workflow_run_id: None,
                effect_id: None,
                trace_id: None,
            };
            if publisher.sender.try_send(record).is_err() {
                publisher.queued_bytes.fetch_sub(size, Ordering::Relaxed);
                publisher.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
