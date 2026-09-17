#[allow(unused_imports)]
use super::*;

pub(super) struct ServiceUrlEnvGuard {
    pub(super) original: Option<OsString>,
    pub(super) _guard: MutexGuard<'static, ()>,
}

impl ServiceUrlEnvGuard {
    pub(super) fn set(value: &str) -> Self {
        let guard = env_lock().lock().unwrap();
        let original = env::var_os("RUNINATOR_SERVICE_URL");
        // safety: this test serializes access to the process env inside this crate.
        unsafe {
            env::set_var("RUNINATOR_SERVICE_URL", value);
        }
        Self {
            original,
            _guard: guard,
        }
    }
}

impl Drop for ServiceUrlEnvGuard {
    fn drop(&mut self) {
        // safety: this test serializes access to the process env inside this crate.
        unsafe {
            match &self.original {
                Some(value) => env::set_var("RUNINATOR_SERVICE_URL", value),
                None => env::remove_var("RUNINATOR_SERVICE_URL"),
            }
        }
    }
}
