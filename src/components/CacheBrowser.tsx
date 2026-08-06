import { For, createSignal } from "solid-js";
import type { CacheEntry } from "../types";

interface Props {
  entries: CacheEntry[];
  onClear: () => void;
  onDelete: (layoutId: string) => void;
}

export default function CacheBrowser(props: Props) {
  const [expanded, setExpanded] = createSignal<string | null>(null);

  function toggle(id: string) {
    setExpanded((prev) => (prev === id ? null : id));
  }

  return (
    <div>
      {props.entries.length > 0 && (
        <button class="cache-clear-btn" onClick={props.onClear}>
          Clear All Cache
        </button>
      )}
      <div class="cache-list">
        <For each={props.entries}>
          {(entry) => {
            const isOpen = () => expanded() === entry.layout_id;
            const versionCurrent = entry.cache_version >= 2;
            return (
              <div class="cache-item" onClick={() => toggle(entry.layout_id)}>
                <div class="cache-item-header">
                  <span class="cache-item-id">{entry.layout_id.slice(0, 24)}...</span>
                  <span
                    class="cache-version-badge"
                    data-current={versionCurrent}
                  >
                    v{entry.cache_version}
                  </span>
                  <span class="cache-item-expand">{isOpen() ? "▲" : "▼"}</span>
                </div>
                <div class="cache-item-meta">
                  Model: {entry.model} | Attempts: {entry.compile_attempts}
                </div>
                <div class="cache-item-meta">Created: {entry.created_at}</div>
                {isOpen() && (
                  <div class="cache-item-detail">
                    <div class="cache-detail-row">
                      <span class="cache-detail-label">Layout ID</span>
                      <span class="cache-detail-value mono">{entry.layout_id}</span>
                    </div>
                    <div class="cache-detail-row">
                      <span class="cache-detail-label">Schema</span>
                      <pre class="cache-detail-pre">{entry.schema}</pre>
                    </div>
                    <div class="cache-detail-row">
                      <span class="cache-detail-label">Model</span>
                      <span class="cache-detail-value">{entry.model}</span>
                    </div>
                    <div class="cache-detail-row">
                      <span class="cache-detail-label">Compile Attempts</span>
                      <span class="cache-detail-value">{entry.compile_attempts}</span>
                    </div>
                    <div class="cache-detail-row">
                      <span class="cache-detail-label">Cache Version</span>
                      <span class="cache-detail-value">
                        <span
                          class="cache-version-badge"
                          data-current={versionCurrent}
                        >
                          v{entry.cache_version}
                        </span>
                        {!versionCurrent && (
                          <span style={{ "color": "var(--danger)", "font-size": "11px", "margin-left": "8px" }}>
                            stale — clear and reprocess
                          </span>
                        )}
                      </span>
                    </div>
                    <div class="cache-detail-row">
                      <span class="cache-detail-label">Created</span>
                      <span class="cache-detail-value">{entry.created_at}</span>
                    </div>
                    <div style={{ "margin-top": "8px" }}>
                      <button
                        class="cache-delete-btn"
                        onClick={(e) => {
                          e.stopPropagation();
                          props.onDelete(entry.layout_id);
                        }}
                      >
                        Delete Entry
                      </button>
                    </div>
                  </div>
                )}
              </div>
            );
          }}
        </For>
      </div>
      {props.entries.length === 0 && (
        <div class="cache-empty">No cached layouts yet. Process some PDFs first.</div>
      )}
    </div>
  );
}
