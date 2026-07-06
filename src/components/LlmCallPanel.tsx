import type { LlmCallRecord, LlmAggregateStats } from "../types";

interface Props {
  records: LlmCallRecord[];
}

function computeStats(records: LlmCallRecord[]): LlmAggregateStats {
  const total = records.length;
  const successful = records.filter((r) => r.success).length;
  return {
    total_calls: total,
    successful_calls: successful,
    failed_calls: total - successful,
    total_input_tokens: records.reduce((s, r) => s + r.token_usage.input_tokens, 0),
    total_output_tokens: records.reduce((s, r) => s + r.token_usage.output_tokens, 0),
    total_cost_cents: records.reduce((s, r) => s + r.token_usage.estimated_cost_cents, 0),
    avg_latency_ms: total > 0 ? records.reduce((s, r) => s + r.latency_ms, 0) / total : 0,
  };
}

const CALL_LABELS: Record<string, string> = {
  SchemaDiscovery: "Schema",
  CodeGeneration: "Codegen",
  CompilationFix: "Compile Fix",
  ExtractionFix: "Extract Fix",
};

export default function LlmCallPanel(props: Props) {
  const stats = computeStats(props.records);

  return (
    <div class="llm-panel">
      <h3 class="llm-panel-title">LLM Calls</h3>

      <div class="llm-stats-row">
        <span class="llm-stat">{stats.total_calls} calls</span>
        <span class="llm-stat ok">{stats.successful_calls} ok</span>
        <span class="llm-stat err">{stats.failed_calls} failed</span>
        <span class="llm-stat">{stats.total_input_tokens} in / {stats.total_output_tokens} out tok</span>
        <span class="llm-stat">${(stats.total_cost_cents / 100).toFixed(2)}</span>
        <span class="llm-stat">{stats.avg_latency_ms.toFixed(0)}ms avg</span>
      </div>

      <div class="llm-call-list">
        {props.records.length === 0 && <div class="llm-empty">No LLM calls yet</div>}
        {props.records.slice(0, 50).map((r) => (
          <div class={`llm-call-row ${r.success ? "" : "llm-call-fail"}`}>
            <span class="llm-call-type">{CALL_LABELS[r.call_type] || r.call_type}</span>
            <span class="llm-call-model">{r.model}</span>
            <span class="llm-call-latency">{r.latency_ms}ms</span>
            <span class="llm-call-tokens">{r.token_usage.input_tokens}→{r.token_usage.output_tokens}</span>
            <span class="llm-call-cost">${(r.token_usage.estimated_cost_cents / 100).toFixed(4)}</span>
            <span class="llm-call-status">{r.success ? "" : `ERR: ${r.error_kind || ""}`}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
