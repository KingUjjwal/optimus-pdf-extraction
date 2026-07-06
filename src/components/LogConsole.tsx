import { For, createEffect } from "solid-js";

interface Props {
  logs: string[];
}

export default function LogConsole({ logs }: Props) {
  let bodyRef!: HTMLDivElement;

  createEffect(() => {
    if (logs.length > 0 && bodyRef) {
      bodyRef.scrollTop = bodyRef.scrollHeight;
    }
  });

  function logClass(line: string): string {
    if (line.includes("ERROR") || line.includes("failed") || line.includes("Fail")) return "error";
    if (line.includes("done") || line.includes("Cache hit") || line.includes("Cache cleared") || line.includes("Cache entry deleted") || line.includes("succeeded")) return "success";
    if (line.startsWith("[pipeline:") || line.includes("Compile failed") || line.includes("LLM fix") || line.includes("Schema inferred")) return "info";
    return "";
  }

  return (
    <>
      <div class="log-header">
        <span>Event Log</span>
        <span style={{ "margin-left": "auto" }}>{logs.length} entries</span>
      </div>
      <div class="log-body" ref={bodyRef!}>
        <For each={logs}>
          {(line) => <div class={`log-line ${logClass(line)}`}>{line}</div>}
        </For>
      </div>
    </>
  );
}
