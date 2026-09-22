import { invoke } from "@tauri-apps/api/core";
import type {
  IngestResult,
  IngestFullResult,
  ExtractionResult,
  CacheEntry,
  SchemaInferenceResult,
  CompileResult,
  LlmCallRecord,
} from "../types";

export async function ingestPdf(path: string): Promise<IngestResult> {
  return invoke("ingest_pdf_command", { path });
}

export async function extractDocument(
  path: string,
  cacheDir: string,
  schema: string | null = null
): Promise<ExtractionResult> {
  return invoke("extract_document_command", { path, cacheDir, schema });
}

export async function discoverSchema(
  spansJson: string,
  customPrompt?: string,
): Promise<string> {
  return invoke("discover_schema_command", { spansJson, customPrompt });
}

export async function compileModule(
  layoutId: string,
  spansJson: string,
  schema: string,
  cacheDir: string,
  flatGraph?: string
): Promise<string> {
  return invoke("compile_module_command", {
    layoutId,
    spansJson,
    schema,
    cacheDir,
    flatGraph,
  });
}

export async function getCacheList(
  cacheDir: string
): Promise<CacheEntry[]> {
  return invoke("get_cache_list_command", { cacheDir });
}

export async function getCacheManifest(
  layoutId: string,
  cacheDir: string
): Promise<string> {
  return invoke("get_cache_manifest_command", { layoutId, cacheDir });
}

export async function clearCache(cacheDir: string): Promise<string> {
  return invoke("clear_cache_command", { cacheDir });
}

export async function deleteCacheEntry(
  layoutId: string,
  cacheDir: string
): Promise<string> {
  return invoke("delete_cache_entry_command", { layoutId, cacheDir });
}

// ── Phase 7: Multi-step wizard commands ──────────────────────────────

export async function ingestDocument(
  path: string,
  cacheDir: string
): Promise<IngestFullResult> {
  return invoke("ingest_command", { path, cacheDir });
}

export async function inferSchemaLLM(
  grid: string,
  spansJson: string,
): Promise<SchemaInferenceResult> {
  return invoke("infer_schema_llm_command", { grid, spansJson });
}

export async function compileModuleLLM(
  layoutId: string,
  spansJson: string,
  schema: string,
  cacheDir: string,
  flatGraph?: string
): Promise<CompileResult> {
  return invoke("compile_module_llm_command", {
    layoutId,
    spansJson,
    schema,
    cacheDir,
    flatGraph,
  });
}

export async function extractCached(
  layoutId: string,
  cacheDir: string
): Promise<ExtractionResult> {
  return invoke("extract_cached_command", { layoutId, cacheDir });
}

export async function getLlmHistory(
  cacheDir: string
): Promise<LlmCallRecord[]> {
  return invoke("get_llm_history_command", { cacheDir });
}
