import { createSignal, Show } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import type { IngestFullResult } from "../types";
import { ingestDocument } from "../lib/commands";

interface Props {
  cacheDir: string;
  onIngested: (result: IngestFullResult) => void;
  initialResult?: IngestFullResult | null;
}

export default function Step0_Upload({ cacheDir, onIngested, initialResult }: Props) {
  const [ingestResult, setIngestResult] = createSignal<IngestFullResult | null>(initialResult ?? null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [fileName, setFileName] = createSignal<string>("");

  async function handleSelectFile() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!selected) return;

    const path = selected as string;
    const name = path.split(/[/\\]/).pop() || path;
    setFileName(name);
    setLoading(true);
    setError(null);

    try {
      const result = await ingestDocument(path, cacheDir);
      setIngestResult(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div class="p-6 flex flex-col gap-4">
      <div>
        <h2 class="text-xl font-semibold text-primary">
          Upload Document
        </h2>
        <p class="mt-2 text-base text-muted">
          Select a PDF file to begin extraction.
        </p>
      </div>

      <Show when={!ingestResult()} fallback={
        <div class="p-5 gradient-success rounded-lg">
          <div class="flex items-start gap-3">
            <div class="icon-circle-lg bg-success">✓</div>
            <div class="flex-1">
              <div class="font-semibold text-md text-text mb-1">
                Document Ready
              </div>
              <div class="text-base text-muted mb-2">
                {fileName()}
              </div>

              <div class="stats-grid">
                <div class="stat-card">
                  <div class="stat-label">Text Spans</div>
                  <div class="stat-value">{ingestResult()!.count}</div>
                </div>
                <div class="stat-card">
                  <div class="stat-label">Layout ID</div>
                  <div class="stat-value font-mono text-xs">{ingestResult()!.layout_id.slice(0, 16)}...</div>
                </div>
                <div class="stat-card">
                  <div class="stat-label">Cached</div>
                  <div class="stat-value">{ingestResult()!.is_cached ? "Yes" : "No"}</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      }>
        <div class="p-8 bg-secondary border-dashed border-2 border-light rounded-lg text-muted text-base text-center">
          <div class="mb-2">
            No document loaded yet
          </div>
          <button
            class="btn btn-primary mt-3"
            onClick={handleSelectFile}
            disabled={loading()}
          >
            {loading() ? "Processing..." : "Select PDF"}
          </button>
        </div>
      </Show>

      <Show when={error()}>
        <div class="alert alert-danger">
          {error()}
          <button class="btn btn-secondary mt-2" onClick={() => setError(null)}>
            Retry
          </button>
        </div>
      </Show>

      <Show when={ingestResult()}>
        <button
          class="btn btn-primary btn-block"
          onClick={() => onIngested(ingestResult()!)}
        >
          Continue to Schema →
        </button>
      </Show>
    </div>
  );
}
