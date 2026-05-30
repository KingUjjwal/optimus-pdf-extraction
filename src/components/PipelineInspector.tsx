import { For, createMemo } from "solid-js";

interface Props {
  steps: { type: string; payload: Record<string, unknown>; ts: number }[];
  layoutId: string;
}

const PIPELINE_DEF = [
  { type: "ingest-start", label: "Ingestion" },
  { type: "ingest-done", label: "Span Extraction" },
  { type: "graph-built", label: "Spatial Graph" },
  { type: "layout-hash", label: "Layout Fingerprint" },
  { type: "compiling", label: "Compilation" },
  { type: "compiled", label: "WASM Compiled" },
  { type: "extracting", label: "Extraction" },
  { type: "done", label: "Complete" },
];

export default function PipelineInspector({ steps, layoutId }: Props) {
  const stepMap = createMemo(() => {
    const map: Record<string, { payload: Record<string, unknown>; ts: number }> = {};
    for (const s of steps) map[s.type] = s;
    return map;
  });

  const lastStep = createMemo(() => {
    const done = stepMap().done;
    const extracted = stepMap().extracted;
    if (done) return "done";
    if (extracted) return "extracted";
    if (stepMap().compiled) return "compiled";
    if (stepMap().compiling) return "compiling";
    if (stepMap()["layout-hash"]) return "layout-hash";
    if (stepMap()["graph-built"]) return "graph-built";
    if (stepMap()["ingest-done"]) return "ingest-done";
    if (stepMap()["ingest-start"]) return "ingest-start";
    return null;
  });

  return (
    <div class="pipeline">
      {layoutId && (
        <div style={{ "font-family": "var(--font-mono)", "font-size": "11px", color: "var(--text-muted)", "margin-bottom": "8px" }}>
          Layout: <span style={{ color: "var(--primary)" }}>{layoutId.slice(0, 16)}...</span>
        </div>
      )}
      <For each={PIPELINE_DEF}>
        {(step) => {
          const info = stepMap()[step.type];
          const isDone = !!info;
          const isLast = lastStep() === step.type;
          return (
            <div class={`pipeline-step ${isDone ? "done" : ""} ${isLast ? "active" : ""}`}>
              <span>{isDone ? "✓" : "○"}</span>
              <span class="step-label">{step.label}</span>
              <span class="step-detail">
                {info?.payload.layout_id
                  ? `ID: ${(info.payload.layout_id as string).slice(0, 10)}`
                  : info?.payload.spans
                  ? `${info.payload.spans} spans`
                  : info?.payload.nodes
                  ? `${info.payload.nodes} nodes`
                  : info?.payload.size_bytes
                  ? `${info.payload.size_bytes}B`
                  : info?.payload.duration_ms
                  ? `${info.payload.duration_ms}ms`
                  : ""}
              </span>
              <span class="step-time">{info?.payload.duration_ms ? `${info.payload.duration_ms}ms` : "—"}</span>
            </div>
          );
        }}
      </For>
      {steps.length === 0 && (
        <div class="no-data" style={{ "padding": "20px", "font-size": "13px" }}>
          Drop a PDF to begin the pipeline
        </div>
      )}
    </div>
  );
}
