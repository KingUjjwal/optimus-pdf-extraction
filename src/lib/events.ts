import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function onPipelineEvent(
  cb: (type: string, payload: Record<string, unknown>) => void
): Promise<UnlistenFn> {
  const events: [string, string][] = [
    ["pipeline:ingest-start", "ingest-start"],
    ["pipeline:ingest-done", "ingest-done"],
    ["pipeline:graph-built", "graph-built"],
    ["pipeline:layout-hash", "layout-hash"],
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

  const listeners: Promise<UnlistenFn>[] = events.map(([eventName, eventType]) =>
    listen<string>(eventName, (e) =>
      cb(eventType, JSON.parse(e.payload))
    )
  );

  return Promise.all(listeners).then((unlisteners) => {
    return () => unlisteners.forEach((u) => u());
  });
}
