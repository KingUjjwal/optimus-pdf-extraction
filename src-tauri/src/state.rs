use optimus_runtime::WasmHost;
use std::sync::Arc;

/// Shared application state. The `WasmHost` (wasmtime Engine + compiled-module
/// cache) is expensive to construct, so one instance is shared across every
/// command and every Rayon worker instead of building a fresh host per call.
pub struct AppState {
    pub host: Arc<WasmHost>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            host: Arc::new(WasmHost::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
