pub mod types;
pub mod ingest;
pub mod extract;
pub mod cache;
pub mod schema;
pub mod compile;
pub mod batch;
pub mod llm;

#[allow(unused_imports)]
pub use types::*;
pub use ingest::*;
pub use extract::*;
pub use cache::*;
pub use schema::*;
pub use compile::*;
pub use batch::*;
pub use llm::*;

use tauri::Emitter;

#[tracing::instrument(level = "debug", skip(app, payload))]
pub(crate) fn emit_event(app: &tauri::AppHandle, event: &str, payload: serde_json::Value) {
    if let Err(e) = app.emit(event, payload.to_string()) {
        log::warn!("Failed to emit event '{}': {}", event, e);
    }
}
