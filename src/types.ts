export interface TextSpan {
  text: string;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export interface Bounds {
  min_x: number;
  max_x: number;
  min_y: number;
  max_y: number;
}

export interface IngestResult {
  spans: TextSpan[];
  count: number;
}

export interface IngestFullResult {
  spans: TextSpan[];
  count: number;
  grid: string;
  flat_graph: string;
  layout_id: string;
  is_cached: boolean;
  bounding_box: Bounds;
}

export interface ExtractedRecord {
  [key: string]: string;
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

export type TabId = "wizard" | "ingest" | "pipeline" | "graph" | "schema" | "cache" | "output";

export type PipelineStage = "idle" | "ingested" | "format-selected" | "schema-defined" | "compiled" | "extracted" | "error";
