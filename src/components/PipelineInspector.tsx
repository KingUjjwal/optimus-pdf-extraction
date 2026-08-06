import { For, createMemo } from "solid-js";

interface Props {
  steps: { type: string; payload: Record<string, unknown>; ts: number }[];
  layoutId: string;
}

const PIPELINE_DEF = [
  { type: "ingest-start", label: "Ingest", hint: "Open PDF, parse pages" },
  { type: "ingest-done", label: "Span Extraction", hint: "Text spans located" },
  { type: "graph-built", label: "Spatial Graph", hint: "R-Tree index built" },
  { type: "layout-hash", label: "Layout Fingerprint", hint: "BLAKE3 anchor vector" },
  { type: "compiling", label: "Compilation", hint: "WASM module build" },
  { type: "compiled", label: "WASM Compiled", hint: "Module ready in cache" },
  { type: "extracting", label: "Extraction", hint: "Running WASM extractor" },
  { type: "extracted", label: "Record Built", hint: "Fields + transactions" },
  { type: "done", label: "Complete", hint: "Pipeline finished" },
];

function stepDetail(payload?: Record<string, unknown>): string {
  if (!payload) return "";
  if (payload.layout_id) return `id ${String(payload.layout_id).slice(0, 12)}`;
  if (payload.spans) return `${payload.spans} spans`;
  if (payload.nodes) return `${payload.nodes} nodes`;
  if (payload.size_bytes) return `${payload.size_bytes} B`;
  if (payload.duration_ms) return `${payload.duration_ms} ms`;
  return "";
}

export default function PipelineInspector(props: Props) {
  const stepMap = createMemo(() => {
    const map: Record<string, { payload: Record<string, unknown>; ts: number }> = {};
    for (const s of props.steps) map[s.type] = s;
    return map;
  });

  const lastIndex = createMemo(() => {
    let idx = -1;
    for (const [i, def] of PIPELINE_DEF.entries()) {
      if (stepMap()[def.type]) idx = i;
    }
    return idx;
  });

  const elapsed = createMemo(() => {
    if (props.steps.length < 2) return null;
    return props.steps[props.steps.length - 1].ts - props.steps[0].ts;
  });

  return (
    <div class="pipeline-card">
      <div class="pipeline-head">
        <h3 class="panel-title">Pipeline</h3>
        {props.layoutId && (
          <span class="pipeline-layout" title={props.layoutId}>
            Layout <span class="mono">{props.layoutId.slice(0, 16)}…</span>
          </span>
        )}
        {elapsed() !== null && (
          <span class="pipeline-layout" title="Total pipeline time">
            {elapsed()} ms
          </span>
        )}
      </div>

      {props.steps.length === 0 ? (
        <div class="no-data" style={{ padding: "24px", "font-size": "13px" }}>
          Drop a PDF to begin the pipeline.
        </div>
      ) : (
        <div class="timeline">
          <For each={PIPELINE_DEF}>
            {(def, index) => {
              const info = stepMap()[def.type];
              const done = !!info;
              const isActive = done && index() === lastIndex();
              const isPending = !done;
              const durMs = typeof info?.payload.duration_ms === "number" ? info.payload.duration_ms : undefined;
              return (
                <div class={`timeline-step ${done ? "done" : ""} ${isActive ? "active" : ""}`}>
                  <div class="timeline-rail">
                    <span class={`timeline-node ${done ? "done" : ""} ${isActive ? "active" : ""}`}>
                      {done && (
                        <svg viewBox="0 0 24 24" class="timeline-check" aria-hidden="true">
                          <path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
                        </svg>
                      )}
                    </span>
                    {index() < PIPELINE_DEF.length - 1 && (
                      <span class={`timeline-line ${done ? "filled" : ""}`} />
                    )}
                  </div>
                  <div class="timeline-body">
                    <div class="timeline-label">
                      {def.label}
                      {isActive && (
                        <span class="badge badge-primary" style={{ "margin-left": "8px", "font-size": "9px", "padding": "1px 7px" }}>
                          live
                        </span>
                      )}
                    </div>
                    <div class="timeline-hint">{isPending ? def.hint : stepDetail(info?.payload) || def.hint}</div>
                  </div>
                  {durMs !== undefined && <span class="timeline-time">{durMs} ms</span>}
                </div>
              );
            }}
          </For>
        </div>
      )}
    </div>
  );
}
