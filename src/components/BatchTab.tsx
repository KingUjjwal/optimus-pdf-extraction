import { createSignal, Show, For } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import type { ExtractedRecord } from "../types";
import ArrowTableView from "./ArrowTableView";

interface Props {
  cacheDir: string;
  addLog: (msg: string) => void;
}

interface BatchEntry {
  path: string;
  file: string;
  layout_id: string;
  was_cached: boolean;
  success: boolean;
  error?: string;
  duration_ms: number;
  fields?: ExtractedRecord;
}

export default function BatchTab({ cacheDir, addLog }: Props) {
  const [files, setFiles] = createSignal<string[]>([]);
  const [running, setRunning] = createSignal(false);
  const [results, setResults] = createSignal<BatchEntry[]>([]);
  const [summary, setSummary] = createSignal<{ total: number; succeeded: number; failed: number; cached: number } | null>(null);

  async function handleSelectFiles() {
    const selected = await open({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setFiles(paths as string[]);
    setResults([]);
    setSummary(null);
  }

  async function handleRun() {
    if (files().length === 0) return;
    setRunning(true);
    setResults([]);
    setSummary(null);

    try {
      const res = await invoke("batch_extract_command", { paths: files(), cacheDir });
      const data = res as any;
      setResults(data.entries || []);
      setSummary({ total: data.total, succeeded: data.succeeded, failed: data.failed, cached: data.cached });
    } catch (e) {
      addLog(`Batch failed: ${e}`);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div class="p-6 flex flex-col gap-4">
      <div>
        <h2 class="text-xl font-semibold text-primary">Batch Extraction</h2>
        <p class="mt-2 text-base text-muted">
          Select multiple PDF files and extract them in one run.
        </p>
      </div>

      <div class="flex gap-2">
        <button class="btn btn-secondary" onClick={handleSelectFiles} disabled={running()}>
          Select PDFs
        </button>
        <button class="btn btn-primary" onClick={handleRun} disabled={running() || files().length === 0}>
          {running() ? "Extracting..." : `Extract ${files().length} file(s)`}
        </button>
      </div>

      <Show when={files().length > 0}>
        <div class="card p-3">
          <div class="text-sm text-muted mb-1">Selected ({files().length})</div>
          <For each={files()}>
            {(f) => <div class="text-xs font-mono">{f.split(/[/\\]/).pop()}</div>}
          </For>
        </div>
      </Show>

      <Show when={summary()}>
        <div class="card p-3 flex gap-4">
          <div><span class="stat-label">Total</span><span class="stat-value ml-1">{summary()!.total}</span></div>
          <div><span class="stat-label text-success">OK</span><span class="stat-value ml-1">{summary()!.succeeded}</span></div>
          <Show when={summary()!.failed > 0}>
            <div><span class="stat-label text-error">Fail</span><span class="stat-value ml-1">{summary()!.failed}</span></div>
          </Show>
          <div><span class="stat-label">Cached</span><span class="stat-value ml-1">{summary()!.cached}</span></div>
        </div>
      </Show>

      <Show when={results().length > 0}>
        <div class="flex flex-col gap-2">
          <For each={results()}>
            {(r) => (
              <div class={`card p-3 ${r.success ? "" : "border-error"}`}>
                <div class="flex gap-2 items-center text-sm">
                  <span class={`text-xs ${r.success ? "text-success" : "text-error"}`}>
                    {r.success ? "✓" : "✗"}
                  </span>
                  <span class="font-mono text-xs flex-1">{r.file}</span>
                  <span class="text-xs text-muted">{r.was_cached ? "cached" : "compiled"}</span>
                  <span class="text-xs text-muted">{r.duration_ms}ms</span>
                  <Show when={r.layout_id}>
                    <span class="text-xs text-muted">{r.layout_id.slice(0, 8)}</span>
                  </Show>
                </div>
                <Show when={!r.success && r.error}>
                  <div class="text-xs text-error mt-1">{r.error!.slice(0, 120)}</div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
