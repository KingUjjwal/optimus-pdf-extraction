import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function onPipelineEvent(
  cb: (type: string, payload: Record<string, unknown>) => void
): Promise<UnlistenFn> {
  const events: [string, string][] = [
    ["pipeline:ingest-start", "ingest-start"],
    ["pipeline:ingest-done", "ingest-done"],
    ["pipeline:graph-built", "graph-built"],
    ["pipeline:layout-hash", "layout-hash"],
    ["pipeline:classify-done", "classify-done"],
    ["pipeline:compiling", "compiling"],
    ["pipeline:compiled", "compiled"],
    ["pipeline:extracting", "extracting"],
    ["pipeline:extracted", "extracted"],
    ["pipeline:done", "done"],
    ["cache:cleared", "cache-cleared"],
    ["pipeline:compile-attempt", "compile-attempt"],
    ["pipeline:schema-inferred", "schema-inferred"],
    ["pipeline:code-generated", "code-generated"],
    ["pipeline:llm-fix", "llm-fix"],
    ["cache:entry-deleted", "cache-entry-deleted"],
    ["llm:call", "llm-call"],
  ];

  function parsePayload(raw: unknown): Record<string, unknown> {
    if (typeof raw === "string") {
      try {
        const parsed = JSON.parse(raw);
        return parsed && typeof parsed === "object" && !Array.isArray(parsed)
          ? (parsed as Record<string, unknown>)
          : { value: parsed };
      } catch {
        return { value: raw };
      }
    }
    if (raw && typeof raw === "object" && !Array.isArray(raw)) {
      return raw as Record<string, unknown>;
    }
    return { value: raw };
  }

  const listeners: Promise<UnlistenFn>[] = events.map(([eventName, eventType]) =>
    listen<unknown>(eventName, (e) => cb(eventType, parsePayload(e.payload)))
  );

  return Promise.all(listeners).then((unlisteners) => {
    return () => unlisteners.forEach((u) => u());
  });
}
