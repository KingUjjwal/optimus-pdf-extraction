pub mod batch;
pub mod cache;
pub mod compile;
pub mod extract;
pub mod ingest;
pub mod llm;
pub mod schema;
pub mod types;

pub use batch::*;
pub use cache::*;
pub use compile::*;
pub use extract::*;
pub use ingest::*;
pub use llm::*;
pub use schema::*;
#[allow(unused_imports)]
pub use types::*;

use tauri::Emitter;

#[tracing::instrument(level = "debug", skip(app, payload))]
pub(crate) fn emit_event(app: &tauri::AppHandle, event: &str, payload: serde_json::Value) {
    if let Err(e) = app.emit(event, payload.to_string()) {
        log::warn!("Failed to emit event '{}': {}", event, e);
    }
}
