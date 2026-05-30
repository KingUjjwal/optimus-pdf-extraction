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
    if (line.includes("ERROR")) return "error";
    if (line.includes("done") || line.includes("Cache hit")) return "success";
    if (line.startsWith("[pipeline:")) return "info";
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
