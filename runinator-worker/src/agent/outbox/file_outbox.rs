#[allow(unused_imports)]
use super::*;

pub struct FileOutbox {
    pub(super) path: PathBuf,
    pub(super) dead_letter_path: PathBuf,
    pub(super) max_entries: usize,
    pub(super) max_bytes: u64,
    pub(super) max_attempts: u32,
    pub(super) state: Mutex<FileState>,
}

impl FileOutbox {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, OutboxError> {
        Self::with_limits(
            path,
            DEFAULT_MAX_ENTRIES,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_ATTEMPTS,
        )
    }

    pub fn with_limits(
        path: impl Into<PathBuf>,
        max_entries: usize,
        max_bytes: u64,
        max_attempts: u32,
    ) -> Result<Self, OutboxError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let dead_letter_path = path.with_extension("dead-letter.jsonl");
        let raw = match fs::read(&path) {
            Ok(raw) => {
                set_private_file_permissions(&path)?;
                raw
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        let mut entries = VecDeque::new();
        for (line_number, line) in raw.split(|byte| *byte == b'\n').enumerate() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let entry = serde_json::from_slice(line).map_err(|err| {
                OutboxError::InvalidData(format!("line {}: {err}", line_number + 1))
            })?;
            entries.push_back(entry);
        }
        Ok(Self {
            path,
            dead_letter_path,
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1),
            max_attempts: max_attempts.max(1),
            state: Mutex::new(FileState {
                entries,
                bytes: raw.len() as u64,
            }),
        })
    }

    pub(super) fn rewrite(&self, state: &mut FileState) -> Result<(), OutboxError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let temp = parent.join(format!(
            ".{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("result-outbox"),
            Uuid::new_v4()
        ));
        let result = (|| -> Result<u64, OutboxError> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            set_private_file_permissions(&temp)?;
            let mut bytes = 0u64;
            for entry in &state.entries {
                let encoded = serde_json::to_vec(entry)
                    .map_err(|err| OutboxError::InvalidData(err.to_string()))?;
                file.write_all(&encoded)?;
                file.write_all(b"\n")?;
                bytes = bytes.saturating_add(encoded.len() as u64 + 1);
            }
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            sync_directory(parent)?;
            Ok(bytes)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        state.bytes = result?;
        Ok(())
    }

    pub(super) fn append_dead_letter(&self, entry: &OutboxEntry) -> Result<(), OutboxError> {
        let encoded =
            serde_json::to_vec(entry).map_err(|err| OutboxError::InvalidData(err.to_string()))?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.dead_letter_path)?;
        set_private_file_permissions(&self.dead_letter_path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        sync_directory(
            self.dead_letter_path
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )?;
        Ok(())
    }
}

impl ResultOutbox for FileOutbox {
    fn append_effect(&self, message: EffectResultMessage) -> Result<(), OutboxError> {
        self.append_message(OutboxMessage(message))
    }

    fn next(&self) -> Result<Option<OutboxEntry>, OutboxError> {
        Ok(self
            .state
            .lock()
            .expect("result outbox lock poisoned")
            .entries
            .front()
            .cloned())
    }

    fn acknowledge(&self, id: Uuid) -> Result<(), OutboxError> {
        let mut state = self.state.lock().expect("result outbox lock poisoned");
        if state.entries.front().is_some_and(|entry| entry.id == id) {
            state.entries.pop_front();
            self.rewrite(&mut state)?;
        }
        Ok(())
    }

    fn record_failure(&self, id: Uuid, error: String) -> Result<(), OutboxError> {
        let mut state = self.state.lock().expect("result outbox lock poisoned");
        let Some(entry) = state.entries.front_mut().filter(|entry| entry.id == id) else {
            return Ok(());
        };
        entry.attempts = entry.attempts.saturating_add(1);
        entry.last_error = Some(error);
        if entry.attempts >= self.max_attempts {
            let dead_letter = entry.clone();
            self.append_dead_letter(&dead_letter)?;
            state.entries.pop_front();
        }
        self.rewrite(&mut state)
    }

    fn depth(&self) -> u64 {
        self.state
            .lock()
            .expect("result outbox lock poisoned")
            .entries
            .len() as u64
    }

    fn is_full(&self) -> bool {
        let state = self.state.lock().expect("result outbox lock poisoned");
        state.entries.len() >= self.max_entries || state.bytes >= self.max_bytes
    }
}

impl FileOutbox {
    pub(super) fn append_message(&self, message: OutboxMessage) -> Result<(), OutboxError> {
        let mut entry = OutboxEntry {
            id: Uuid::now_v7(),
            message,
            attempts: 0,
            last_error: None,
        };
        let encoded =
            serde_json::to_vec(&entry).map_err(|err| OutboxError::InvalidData(err.to_string()))?;
        let mut state = self.state.lock().expect("result outbox lock poisoned");
        if state.entries.len() >= self.max_entries
            || state.bytes.saturating_add(encoded.len() as u64 + 1) > self.max_bytes
        {
            // work already in flight must never be nacked and re-executed merely because older
            // results filled the pending queue. durably dead-letter this overflow, leave the queue
            // full (which puts the agent into draining), and acknowledge only after this fsync.
            entry.attempts = self.max_attempts;
            entry.last_error = Some("result outbox capacity exceeded".to_string());
            self.append_dead_letter(&entry)?;
            return Ok(());
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        set_private_file_permissions(&self.path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        sync_directory(self.path.parent().unwrap_or_else(|| Path::new(".")))?;
        state.bytes = state.bytes.saturating_add(encoded.len() as u64 + 1);
        state.entries.push_back(entry);
        Ok(())
    }
}
