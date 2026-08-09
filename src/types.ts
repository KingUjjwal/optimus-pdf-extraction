export interface TextSpan {
  text: string;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  page?: number | null;
}

export interface Bounds {
  min_x: number;
  max_x: number;
  min_y: number;
  max_y: number;
}

export interface TextQualityReport {
  has_encoding_issues: boolean;
  pages_needing_ocr: number[];
  reasons_by_page: Record<string, string[]>;
}

export interface PdfTypeResult {
  pdf_type: string;
  page_count: number;
  pages_sampled: number;
  pages_with_text: number;
  confidence: number;
  ocr_recommended: boolean;
  pages_needing_ocr: number[];
  ocr_reasons_by_page: Record<string, string[]>;
}

export interface IngestResult {
  spans: TextSpan[];
  count: number;
  quality: TextQualityReport;
}

export interface IngestFullResult {
  spans: TextSpan[];
  count: number;
  grid: string;
  flat_graph: string;
  layout_id: string;
  is_cached: boolean;
  bounding_box: Bounds;
  quality: TextQualityReport;
  classification: PdfTypeResult;
  needs_ocr: boolean;
}

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface ExtractedRecord {
  [key: string]: unknown;
}

export interface ExtractionResult {
  record: ExtractedRecord;
  spans: TextSpan[];
  layout_id: string;
  was_cached: boolean;
  duration_ms: number;
}

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  estimated_cost_cents: number;
}

export interface SchemaInferenceResult {
  schema: string;
  provider_used: string;
  token_usage: TokenUsage;
  fallback: boolean;
}

export interface CompileResult {
  size_bytes: number;
  compile_attempts: number;
  extraction_attempts: number;
  llm_fix_attempts: number;
  token_usage: TokenUsage;
}

export interface CacheEntry {
  layout_id: string;
  schema: string;
  model: string;
  compile_attempts: number;
  created_at: string;
  cache_version: number;
}

export interface PipelineEvent {
  type: string;
  payload: Record<string, unknown>;
}

export type TabId = "wizard" | "ingest" | "pipeline" | "graph" | "schema" | "cache" | "output" | "settings" | "batch";

export type PipelineStage = "idle" | "ingested" | "format-selected" | "schema-defined" | "compiled" | "extracted" | "error";

export interface LlmCallRecord {
  call_type: string;
  model: string;
  system_chars: number;
  user_chars: number;
  response_chars: number;
  latency_ms: number;
  token_usage: TokenUsage;
  success: boolean;
  error_kind: string | null;
  error_message: string | null;
  timestamp: string;
}

export interface LlmAggregateStats {
  total_calls: number;
  successful_calls: number;
  failed_calls: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost_cents: number;
  avg_latency_ms: number;
}
