use libloading::{Library, Symbol};
use log::{debug, warn};
use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::ProviderMetadata,
    runs::{
        ProviderExecutionEvent, ProviderExecutionRequest, ProviderExecutionResponse,
        TaskExecutionResult,
    },
};
use runinator_utilities::ffiutils;
use std::{
    ffi::{CString, c_char, c_int},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crate::provider::{Provider, ProviderEventSink};

const PLUGIN_MARKER_FN_NAME: &str = "runinator_marker\0";
const PLUGIN_NAME_FN_NAME: &str = "name\0";
const PLUGIN_METADATA_FN_NAME: &str = "metadata\0";
const PLUGIN_ABI_VERSION_FN_NAME: &str = "runinator_abi_version\0";
const PLUGIN_SERVICE_CALL_FN_NAME: &str = "call_service\0";

type PluginMarkerFn = unsafe extern "C" fn() -> c_int;
type PluginNameFn = unsafe extern "C" fn() -> *const c_char;
type PluginMetadataFn = unsafe extern "C" fn() -> *const c_char;
type PluginAbiVersionFn = unsafe extern "C" fn() -> c_int;
type PluginServiceCallFn = unsafe extern "C" fn(
    request_json_path: *const c_char,
    response_json_path: *const c_char,
) -> c_int;

#[derive(Clone)]
pub struct Plugin {
    pub file_name: PathBuf,
    pub name: String,
}

impl Provider for Plugin {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn metadata(&self) -> ProviderMetadata {
        self.plugin_metadata().unwrap_or_else(|err| {
            warn!("Failed to load plugin metadata for {}: {}", self.name, err);
            ProviderMetadata {
                name: self.name.clone(),
                actions: vec![],
                metadata: Default::default(),
            }
        })
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        self.plugin_service_call(request, sink)
    }
}

impl Plugin {
    pub fn new(path: &PathBuf) -> Result<Self, SendableError> {
        let lib = unsafe { Library::new(path)? };

        let marker_symbol: Symbol<PluginMarkerFn> =
            unsafe { lib.get(PLUGIN_MARKER_FN_NAME.as_bytes())? };

        let name_symbol: Symbol<PluginNameFn> = unsafe { lib.get(PLUGIN_NAME_FN_NAME.as_bytes())? };

        let marker_result = unsafe { (marker_symbol)() };
        if marker_result != 1 {
            return Err(Box::new(RuntimeError::new(
                "1".to_string(),
                "Marker function did not return expected value".to_string(),
            )));
        }

        let version_symbol: Symbol<PluginAbiVersionFn> =
            unsafe { lib.get(PLUGIN_ABI_VERSION_FN_NAME.as_bytes())? };
        let abi_version = unsafe { (version_symbol)() };
        if abi_version < 1 {
            return Err(Box::new(RuntimeError::new(
                "plugin.abi.unsupported".to_string(),
                format!("Plugin ABI version {abi_version} is not supported; ABI 1 is required"),
            )));
        }
        let _: Symbol<PluginServiceCallFn> =
            unsafe { lib.get(PLUGIN_SERVICE_CALL_FN_NAME.as_bytes())? };

        let name = unsafe { name_symbol() };
        let name_str_buf = ffiutils::cstr_to_rust_string(name);

        Ok(Plugin {
            name: name_str_buf,
            file_name: path.clone(),
        })
    }

    fn plugin_service_call(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let request_path = unique_temp_file("request", "json");
        let response_path = unique_temp_file("response", "json");

        if let Some(parent) = request_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = PathBuf::from(&request.events_jsonl_path).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir_all(&request.artifact_dir)?;
        fs::write(&request_path, serde_json::to_vec_pretty(&request)?)?;

        let stop = Arc::new(AtomicBool::new(false));
        let poller = sink.map(|sink| {
            let events_path = PathBuf::from(request.events_jsonl_path.clone());
            let stop = Arc::clone(&stop);
            thread::spawn(move || poll_events_until_stopped(events_path, sink, stop))
        });

        let result = unsafe {
            let lib = Library::new(self.file_name.clone())?;
            let service_call_symbol: Symbol<PluginServiceCallFn> =
                lib.get(PLUGIN_SERVICE_CALL_FN_NAME.as_bytes())?;
            let request_cstr = CString::new(request_path.to_string_lossy().as_bytes()).unwrap();
            let response_cstr = CString::new(response_path.to_string_lossy().as_bytes()).unwrap();
            (service_call_symbol)(request_cstr.as_ptr(), response_cstr.as_ptr())
        };

        stop.store(true, Ordering::Relaxed);
        if let Some(poller) = poller {
            if let Ok(Err(err)) = poller.join() {
                warn!("Plugin event poller failed: {}", err);
            }
        }

        if result != 0 {
            return Err(Box::new(RuntimeError::new(
                "plugin.v2.execution_failed".to_string(),
                "Plugin execution failed".to_string(),
            )));
        }

        let response_file = File::open(&response_path)?;
        let response: ProviderExecutionResponse = serde_json::from_reader(response_file)?;
        Ok(response.into())
    }

    fn plugin_metadata(&self) -> Result<ProviderMetadata, SendableError> {
        let lib = unsafe { Library::new(self.file_name.clone())? };
        let metadata_symbol: Symbol<PluginMetadataFn> =
            unsafe { lib.get(PLUGIN_METADATA_FN_NAME.as_bytes())? };
        let metadata = unsafe { metadata_symbol() };
        let metadata = ffiutils::cstr_to_rust_string(metadata);
        let mut metadata: ProviderMetadata = serde_json::from_str(&metadata)?;
        if metadata.name.trim().is_empty() {
            metadata.name = self.name.clone();
        }
        Ok(metadata)
    }
}

fn unique_temp_file(kind: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join("runinator-plugin").join(format!(
        "{}-{}-{}.{}",
        kind,
        std::process::id(),
        chrono_like_now(),
        extension
    ))
}

fn chrono_like_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn poll_events_until_stopped(
    events_path: PathBuf,
    sink: Arc<dyn ProviderEventSink>,
    stop: Arc<AtomicBool>,
) -> Result<(), SendableError> {
    let mut offset = 0;
    while !stop.load(Ordering::Relaxed) {
        offset = poll_events_once(&events_path, offset, sink.as_ref())?;
        thread::sleep(Duration::from_millis(100));
    }
    poll_events_once(&events_path, offset, sink.as_ref())?;
    Ok(())
}

fn poll_events_once(
    events_path: &PathBuf,
    offset: u64,
    sink: &dyn ProviderEventSink,
) -> Result<u64, SendableError> {
    let Ok(mut file) = OpenOptions::new().read(true).open(events_path) else {
        return Ok(offset);
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut current_offset = offset;
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        current_offset += bytes as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<ProviderExecutionEvent>(trimmed) {
            Ok(event) => sink.emit(event),
            Err(err) => debug!("Ignoring malformed plugin event: {}", err),
        }
    }
    Ok(current_offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::runs::ProviderExecutionEvent;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemorySink {
        events: Mutex<Vec<ProviderExecutionEvent>>,
    }

    impl ProviderEventSink for MemorySink {
        fn emit(&self, event: ProviderExecutionEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn polls_valid_events_and_skips_malformed_lines() {
        let events_path = unique_temp_file("events-test", "jsonl");
        fs::create_dir_all(events_path.parent().unwrap()).unwrap();
        fs::write(
            &events_path,
            concat!(
                "{\"type\":\"chunk\",\"stream\":\"stdout\",\"content\":\"hello\"}\n",
                "not json\n",
                "{\"type\":\"message\",\"message\":\"done\"}\n",
            ),
        )
        .unwrap();

        let sink = MemorySink::default();
        let offset = poll_events_once(&events_path, 0, &sink).unwrap();
        assert!(offset > 0);

        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ProviderExecutionEvent::Chunk { stream, content }
                if stream == "stdout" && content == "hello"
        ));
        assert!(matches!(
            &events[1],
            ProviderExecutionEvent::Message { message } if message == "done"
        ));
    }
}
