import { createSignal, createMemo, onMount, onCleanup, Show, For } from "solid-js";
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

interface TabDef {
  id: TabId;
  label: string;
  paths: string[];
  mode?: "fill" | "stroke";
}

const TABS: TabDef[] = [
  { id: "wizard", label: "Wizard", mode: "fill", paths: ["M12 2.5l2.6 6.4 6.9.5-5.3 4.5 1.7 6.7L12 17l-5.9 3.6 1.7-6.7L2.5 9.4l6.9-.5L12 2.5z"] },
  { id: "ingest", label: "Ingest", mode: "fill", paths: ["M12 15V4m0 0L7 9m5-5l5 5", "M4 19h16"] },
  { id: "pipeline", label: "Pipeline", mode: "fill", paths: ["M13 2L4 14h6l-1 8 9-12h-6l1-8z"] },
  { id: "graph", label: "Spatial Graph", mode: "stroke", paths: [
    "M12 4a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
    "M4 20a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
    "M20 20a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
    "M12 4v6M4 20l6-6M20 20l-6-6",
  ] },
  { id: "schema", label: "Schema", mode: "stroke", paths: ["M8 3H7a2 2 0 0 0-2 2v4a2 2 0 0 1-2 2 2 2 0 0 1 2 2v4a2 2 0 0 0 2 2h1M16 3h1a2 2 0 0 1 2 2v4a2 2 0 0 0 2 2 2 2 0 0 0-2 2v4a2 2 0 0 1-2 2h-1"] },
  { id: "cache", label: "Cache", mode: "stroke", paths: ["M4 6c0 1.7 3.6 3 8 3s8-1.3 8-3-3.6-3-8-3-8 1.3-8 3z", "M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6", "M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"] },
  { id: "output", label: "Output", mode: "stroke", paths: ["M4 4h16v16H4z", "M4 10h16", "M10 4v16"] },
  { id: "batch", label: "Batch", mode: "fill", paths: ["M12 2l10 6-10 6L2 8l10-6z", "M2 16l10 6 10-6", "M2 12l10 6 10-6"] },
  { id: "settings", label: "Settings", mode: "stroke", paths: ["M4 7h16M4 17h16", "M8 4v6", "M16 14v6"] },
];

function TabIcon(props: { paths: string[]; mode?: "fill" | "stroke" }) {
  const stroke = props.mode !== "fill";
  return (
    <svg
      viewBox="0 0 24 24"
      class="tab-icon"
      aria-hidden="true"
      fill={stroke ? "none" : "currentColor"}
      stroke={stroke ? "currentColor" : "none"}
      stroke-width="1.7"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <For each={props.paths}>
        {(d) => <path d={d} />}
      </For>
    </svg>
  );
}

const PIPELINE_PHASES: { label: string; events: string[] }[] = [
  { label: "Ingest", events: ["ingest-start", "ingest-done", "graph-built", "layout-hash"] },
  { label: "Schema", events: ["schema-inferred"] },
  { label: "Compile", events: ["compiling", "compile-attempt", "compiled", "code-generated", "llm-fix"] },
  { label: "Extract", events: ["extracting", "extracted", "done"] },
];

function headerStats(record: ExtractedRecord | null, spans: TextSpan[]): { fields: number; transactions: number; spans: number } {
  let fields = 0;
  let transactions = 0;
  if (record) {
    const entries = Object.entries(record);
    fields = entries.filter(([, v]) => typeof v !== "object" || v === null).length;
    for (const [, v] of entries) {
      if (Array.isArray(v)) transactions += v.length;
    }
  }
  return { fields, transactions, spans: spans.length };
}

export default function App() {
  const [activeTab, setActiveTab] = createSignal<TabId>("wizard");
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
  const [logOpen, setLogOpen] = createSignal(true);

  function addLog(msg: string) {
    setLogs((prev) => [...prev.slice(-500), msg]);
  }

  function handleEvent(type: string, payload: Record<string, unknown>) {
    addLog(`[${type}] ${JSON.stringify(payload).slice(0, 120)}`);
    setPipelineSteps((prev) => [...prev, { type, payload, ts: Date.now() }]);

    if (type === "ingest-start") {
      if (typeof payload.path === "string") setLatestDocumentPath(payload.path);
      setProcessing(true);
    }
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
    if (type === "extracting") {
      setProcessing(true);
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
      addLog(`Extracted: ${JSON.stringify(payload.record).slice(0, 80)}`);
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
    } finally {
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

  const pipelinePhaseIndex = createMemo(() => {
    const types = new Set(pipelineSteps().map((s) => s.type));
    let idx = -1;
    PIPELINE_PHASES.forEach((phase, i) => {
      if (phase.events.some((t) => types.has(t))) idx = i;
    });
    return idx;
  });

  function phaseState(index: number): "done" | "active" | "pending" {
    const current = pipelinePhaseIndex();
    if (index < current) return "done";
    if (index === current) return processing() ? "active" : "done";
    return "pending";
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

  const stats = () => headerStats(latestRecord(), latestSpans());

  return (
    <div class="app-shell">
      <header class="app-header">
        <div class="header-left">
          <div class="brand">
            <div class="brand-mark" aria-hidden="true">
              <span class="brand-mark-core" />
            </div>
            <div class="brand-text">
              <h1 class="app-title">Optimus</h1>
              <span class="app-subtitle">Intelligent Document Extraction</span>
            </div>
          </div>

          <nav class="header-pipeline" aria-label="Pipeline phases">
            <For each={PIPELINE_PHASES}>
              {(phase, i) => {
                const state = phaseState(i());
                return (
                  <>
                    <span class={`phase-chip ${state}`} data-phase={state}>
                      <span class="phase-dot" />
                      {phase.label}
                    </span>
                    {i() < PIPELINE_PHASES.length - 1 && (
                      <span class={`phase-sep ${state === "done" ? "done" : ""}`} />
                    )}
                  </>
                );
              }}
            </For>
          </nav>

          <Show when={latestDocumentPath()}>
            <span class="header-doc" title={latestDocumentPath()}>
              {latestDocumentPath()}
            </span>
          </Show>
        </div>

        <div class="header-actions">
          <Show when={stats().spans > 0}>
            <span class="header-pill" title={`${stats().spans} text spans`}>
              <span class="pill-dot" style={{ background: "var(--primary)" }} />
              {stats().spans} spans
            </span>
          </Show>
          <Show when={stats().fields > 0}>
            <span class="header-pill" title="Extracted fields">
              <span class="pill-dot" style={{ background: "var(--secondary)" }} />
              {stats().fields} fields
            </span>
          </Show>
          <Show when={stats().transactions > 0}>
            <span class="header-pill" title="Extracted transactions">
              <span class="pill-dot" style={{ background: "var(--success)" }} />
              {stats().transactions} transactions
            </span>
          </Show>
          <span class="app-status" data-cached={cacheEntries().length > 0}>
            {cacheEntries().length > 0 ? `${cacheEntries().length} layouts cached` : "no cache"}
          </span>
          <Show when={processing()}>
            <span class="processing-indicator">
              <span class="processing-dot" />
              Processing…
            </span>
          </Show>
        </div>
      </header>

      <nav class="tab-bar">
        {TABS.map((tab) => (
          <button
            class={`tab ${activeTab() === tab.id ? "active" : ""}`}
            onClick={() => setActiveTab(tab.id)}
            aria-current={activeTab() === tab.id ? "page" : undefined}
          >
            <TabIcon paths={tab.paths} mode={tab.mode} />
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

        <div class="tab-content-enter">
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
        </div>
      </main>

      <footer class={`log-panel ${logOpen() ? "" : "collapsed"}`}>
        <LogConsole logs={logs()} onClear={() => setLogs([])} />
        <button class="log-toggle" onClick={() => setLogOpen((v) => !v)} title={logOpen() ? "Collapse log" : "Expand log"}>
          {logOpen() ? "⌄" : "⌃"}
        </button>
      </footer>
    </div>
  );
}
