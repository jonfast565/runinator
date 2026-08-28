//! SDK for filesystem-installed orchestration adapters.

pub use runinator_adapter_contract as contract;

use contract::{AdapterPollRequest, AdapterPollResponse, AdapterRequest, AdapterResponse};
use runinator_models::orchestration::AdapterKindMetadata;

pub trait Adapter: Default {
    fn metadata(&self) -> AdapterKindMetadata;
    fn handle(&self, request: AdapterRequest) -> AdapterResponse;

    /// Polling is opt-in so existing webhook-only dynamic adapters remain source-compatible.
    fn poll(&self, _request: AdapterPollRequest) -> AdapterPollResponse {
        AdapterPollResponse {
            events: Vec::new(),
            checkpoint: serde_json::Value::Null,
            retry_after_seconds: None,
            error: Some("adapter does not support polling".into()),
        }
    }
}

/// Export the stable file-based ABI for an [`Adapter`] implementation.
#[macro_export]
macro_rules! export_adapter {
    ($adapter:ty, $name:literal) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn runinator_adapter_abi_version() -> u32 {
            $crate::contract::ADAPTER_ABI_VERSION
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn runinator_adapter_name() -> *const ::std::ffi::c_char {
            concat!($name, "\0").as_ptr().cast()
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn runinator_adapter_metadata(
            _request_path: *const ::std::ffi::c_char,
            response_path: *const ::std::ffi::c_char,
        ) -> i32 {
            $crate::write_metadata::<$adapter>(response_path)
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn runinator_adapter_handle(
            request_path: *const ::std::ffi::c_char,
            response_path: *const ::std::ffi::c_char,
        ) -> i32 {
            $crate::handle_files::<$adapter>(request_path, response_path)
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn runinator_adapter_poll(
            request_path: *const ::std::ffi::c_char,
            response_path: *const ::std::ffi::c_char,
        ) -> i32 {
            $crate::poll_files::<$adapter>(request_path, response_path)
        }
    };
}

/// Called by the generated ABI wrapper; plugins should use [`export_adapter!`] instead.
pub unsafe fn write_metadata<T: Adapter>(response_path: *const std::ffi::c_char) -> i32 {
    let result = (|| {
        // SAFETY: the adapter host passes a non-null, NUL-terminated path for this invocation.
        let response = unsafe { std::ffi::CStr::from_ptr(response_path) }.to_str()?;
        let envelope = contract::AdapterMetadataEnvelope {
            abi_version: contract::ADAPTER_ABI_VERSION,
            metadata: T::default().metadata(),
        };
        std::fs::write(response, serde_json::to_vec(&envelope)?)?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })();
    if result.is_ok() { 0 } else { 1 }
}

/// Called by the generated ABI wrapper; plugins should use [`export_adapter!`] instead.
pub unsafe fn handle_files<T: Adapter>(
    request_path: *const std::ffi::c_char,
    response_path: *const std::ffi::c_char,
) -> i32 {
    let result = (|| {
        // SAFETY: the adapter host passes non-null, NUL-terminated paths for this invocation.
        let request = unsafe { std::ffi::CStr::from_ptr(request_path) }.to_str()?;
        // SAFETY: same contract as above.
        let response = unsafe { std::ffi::CStr::from_ptr(response_path) }.to_str()?;
        let request: AdapterRequest = serde_json::from_slice(&std::fs::read(request)?)?;
        std::fs::write(response, serde_json::to_vec(&T::default().handle(request))?)?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })();
    if result.is_ok() { 0 } else { 1 }
}

/// Called by the generated ABI wrapper for a durable polling invocation.
pub unsafe fn poll_files<T: Adapter>(
    request_path: *const std::ffi::c_char,
    response_path: *const std::ffi::c_char,
) -> i32 {
    let result = (|| {
        // SAFETY: same bounded-file contract as `handle_files`.
        let request = unsafe { std::ffi::CStr::from_ptr(request_path) }.to_str()?;
        let response = unsafe { std::ffi::CStr::from_ptr(response_path) }.to_str()?;
        let request: AdapterPollRequest = serde_json::from_slice(&std::fs::read(request)?)?;
        std::fs::write(response, serde_json::to_vec(&T::default().poll(request))?)?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })();
    if result.is_ok() { 0 } else { 1 }
}
