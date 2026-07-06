import { createSignal, onMount, onCleanup, Show } from "solid-js";
import type { TabId, TextSpan, ExtractedRecord, CacheEntry, LlmCallRecord } from "./types";
import { appDataDir, join } from "@tauri-apps/api/path";
import { onPipelineEvent } from "./lib/events";
import { discoverSchema, extractDocument, getCacheList, clearCache, deleteCacheEntry, getCacheManifest } from "./lib/commands";
import PdfDropZone from "./components/PdfDropZone";
import PipelineInspector from "./components/PipelineInspector";
import SpatialGraphView from "./components/SpatialGraphView";
import CacheBrowser from "./components/CacheBrowser";
import ArrowTableView from "./components/ArrowTableView";
import SchemaEditor from "./components/SchemaEditor";
import LogConsole from "./components/LogConsole";
import SettingsPanel from "./components/SettingsPanel";
import BatchTab from "./components/BatchTab";
import Wizard from "./components/Wizard";
import LlmCallPanel from "./components/LlmCallPanel";
import "./styles.css";

const TABS: { id: TabId; label: string }[] = [
  { id: "wizard", label: "Wizard" },
  { id: "ingest", label: "Ingest" },
  { id: "pipeline", label: "Pipeline" },
  { id: "graph", label: "Spatial Graph" },
  { id: "schema", label: "Schema" },
  { id: "cache", label: "Cache" },
  { id: "output", label: "Output" },
  { id: "batch", label: "Batch" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [activeTab, setActiveTab] = createSignal<TabId>("ingest");
  const [logs, setLogs] = createSignal<string[]>([]);
  const [latestSpans, setLatestSpans] = createSignal<TextSpan[]>([]);
  const [latestDocumentPath, setLatestDocumentPath] = createSignal<string | undefined>(undefined);
  const [latestRecord, setLatestRecord] = createSignal<ExtractedRecord | null>(null);
  const [latestLayoutId, setLatestLayoutId] = createSignal<string>("");
  const [pipelineSteps, setPipelineSteps] = createSignal<{ type: string; payload: Record<string, unknown>; ts: number }[]>([]);
  const [cacheEntries, setCacheEntries] = createSignal<CacheEntry[]>([]);
  const [extractionSchema, setExtractionSchema] = createSignal<string>('{"invoice_number":"string","date":"string","total":"string"}');
  const [cacheDir, setCacheDir] = createSignal<string>("optimus_cache");
  const [processing, setProcessing] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [llmCallRecords, setLlmCallRecords] = createSignal<LlmCallRecord[]>([]);

  function addLog(msg: string) {
    setLogs((prev) => [...prev.slice(-500), msg]);
  }

  function handleEvent(type: string, payload: Record<string, unknown>) {
    addLog(`[${type}] ${JSON.stringify(payload).slice(0, 120)}`);
    setPipelineSteps((prev) => [...prev, { type, payload, ts: Date.now() }]);

    if (type === "ingest-done" && payload.spans) {
      addLog(`Extracted ${payload.spans} spans in ${payload.duration_ms}ms`);
      if (payload.spans_data) {
        setLatestSpans(payload.spans_data as TextSpan[]);
      }
    }
    if (type === "graph-built") {
      addLog(`Spatial graph: ${payload.nodes} nodes`);
    }
    if (type === "layout-hash") {
      setLatestLayoutId(payload.layout_id as string);
      addLog(`Layout ID: ${payload.layout_id} (cached: ${payload.is_cached})`);
    }
    if (type === "compiling") {
      setProcessing(true);
      addLog(`Compiling module for layout ${payload.layout_id}...`);
    }
    if (type === "compile-attempt") {
      const status = payload.status as string;
      if (status === "failed") {
        addLog(`Compile attempt ${payload.attempt} failed: ${(payload.errors as string || "").slice(0, 100)}`);
      } else if (status === "success") {
        addLog(`Compile attempt ${payload.attempt} succeeded`);
      }
    }
    if (type === "compiled") {
      addLog(`Compiled ${payload.size_bytes} bytes in ${payload.duration_ms}ms`);
    }
    if (type === "extracted" && payload.record) {
      setLatestRecord(payload.record as ExtractedRecord);
      addLog(`Extracted: ${JSON.stringify((payload.record as ExtractedRecord).fields).slice(0, 80)}`);
    }
    if (type === "done") {
      setProcessing(false);
      addLog(`Pipeline done in ${payload.total_duration_ms}ms`);
      refreshCache();
    }
    if (type === "schema-inferred") {
      addLog(`Schema inferred via ${payload.provider}${payload.fallback ? " (fallback)" : ""}`);
      if (payload.schema) setExtractionSchema(payload.schema as string);
    }
    if (type === "code-generated") {
      addLog(`Code generated: ${payload.size_bytes} bytes for layout ${(payload.layout_id as string || "").slice(0, 16)}`);
    }
    if (type === "llm-call") {
      const record = payload as unknown as LlmCallRecord;
      setLlmCallRecords((prev) => [record, ...prev].slice(0, 200));
      addLog(`LLM ${record.call_type}: ${record.model} ${record.latency_ms}ms ${record.success ? "" : "FAILED"}`);
    }
    if (type === "llm-fix") {
      const tokenUsage = payload.token_usage as { estimated_cost_cents?: number } | undefined;
      addLog(`LLM fix applied: ${payload.llm_fix_attempts} attempt(s), cost: ${tokenUsage?.estimated_cost_cents ?? "?"} cents`);
    }
  }

  async function processPdf(path: string) {
    setPipelineSteps([]);
    setLatestRecord(null);
    setLatestDocumentPath(path);
    setError(null);
    setProcessing(true);
    addLog(`Processing: ${path}`);
    try {
      const result = await extractDocument(path, cacheDir(), extractionSchema());
      setLatestRecord(result.record);
      setLatestSpans(result.spans);
      if (result.was_cached) {
        addLog(`Cache hit: ${result.layout_id}`);
      }
    } catch (e) {
      const msg = String(e);
      setError(msg);
      addLog(`ERROR: ${msg}`);
      setProcessing(false);
    }
  }

  async function refreshCache(dir = cacheDir()) {
    try {
      const entries = await getCacheList(dir);
      setCacheEntries(entries);
    } catch (_) {
      /* offline */
    }
  }

  async function checkCacheForLayout(layoutId: string): Promise<boolean> {
    try {
      await getCacheManifest(layoutId, cacheDir());
      return true;
    } catch (_) {
      return false;
    }
  }

  let unlisten: import("@tauri-apps/api/event").UnlistenFn | null = null;
  onMount(async () => {
    onPipelineEvent(handleEvent).then((u) => (unlisten = u));
    
    try {
      const baseDir = await appDataDir();
      const resolvedCacheDir = await join(baseDir, "optimus_cache");
      setCacheDir(resolvedCacheDir);
      refreshCache(resolvedCacheDir);
    } catch (e) {
      addLog(`Failed to resolve appDataDir: ${e}`);
      refreshCache();
    }
  });
  onCleanup(() => unlisten?.());

  return (
    <div class="app-shell">
      <header class="app-header">
        <h1 class="app-title">Optimus</h1>
        <span class="app-subtitle">Intelligent Document Extraction</span>
        <span class="app-status" data-cached={cacheEntries().length > 0}>
          {cacheEntries().length > 0 ? `${cacheEntries().length} layouts cached` : "no cache"}
        </span>
        <Show when={processing()}>
          <span class="processing-indicator">Processing...</span>
        </Show>
      </header>

      <nav class="tab-bar">
        {TABS.map((tab) => (
          <button
            class={`tab ${activeTab() === tab.id ? "active" : ""}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      <main class="main-content">
        <Show when={error()}>
          <div class="error-banner" onClick={() => setError(null)}>
            <span class="error-icon">&#x26A0;</span>
            <span class="error-text">{error()}</span>
            <span class="error-dismiss">click to dismiss</span>
          </div>
        </Show>

        {activeTab() === "wizard" && (
          <Wizard
            schema={extractionSchema()}
            onSchemaChange={setExtractionSchema}
            latestSpans={latestSpans}
            latestDocumentPath={latestDocumentPath}
            latestRecord={latestRecord}
            setLatestRecord={setLatestRecord}
            layoutId={latestLayoutId}
            addLog={addLog}
            checkCacheForLayout={checkCacheForLayout}
            cacheDir={cacheDir()}
          />
        )}
        {activeTab() === "ingest" && (
          <PdfDropZone onFileDrop={processPdf} spans={latestSpans()} processing={processing()} />
        )}
        {activeTab() === "pipeline" && (
          <>
            <PipelineInspector steps={pipelineSteps()} layoutId={latestLayoutId()} />
            <LlmCallPanel records={llmCallRecords()} />
          </>
        )}
        {activeTab() === "graph" && (
          <SpatialGraphView spans={latestSpans()} />
        )}
        {activeTab() === "schema" && (
          <SchemaEditor
            schema={extractionSchema()}
            onSchemaChange={setExtractionSchema}
            onInfer={async (customPrompt) => {
              const spans = latestSpans();
              if (spans.length === 0) {
                addLog("No spans loaded. Drop a PDF first.");
                throw new Error("No spans loaded — process a PDF first");
              }
              const inferred = await discoverSchema(JSON.stringify(spans), customPrompt);
              setExtractionSchema(inferred);
              addLog("Schema inferred from document spans");
            }}
          />
        )}
        {activeTab() === "cache" && (
          <CacheBrowser
            entries={cacheEntries()}
            onClear={async () => {
              await clearCache(cacheDir());
              addLog("Cache cleared");
              refreshCache();
            }}
            onDelete={async (id) => {
              await deleteCacheEntry(id, cacheDir());
              addLog(`Deleted cache entry ${id.slice(0, 16)}`);
              refreshCache();
            }}
          />
        )}
        {activeTab() === "output" && (
          <ArrowTableView record={latestRecord()} />
        )}
        {activeTab() === "batch" && (
          <BatchTab cacheDir={cacheDir()} addLog={addLog} />
        )}
        {activeTab() === "settings" && (
          <SettingsPanel cacheDir={cacheDir()} addLog={addLog} />
        )}
      </main>

      <footer class="log-panel">
        <LogConsole logs={logs()} />
      </footer>
    </div>
  );
}
