import { For, createEffect, createSignal, Show } from "solid-js";

interface Props {
  logs: string[];
  onClear?: () => void;
}

type Level = "all" | "error" | "success" | "info";

export default function LogConsole(props: Props) {
  let bodyRef!: HTMLDivElement;
  const [level, setLevel] = createSignal<Level>("all");

  createEffect(() => {
    if (props.logs.length > 0 && bodyRef) {
      bodyRef.scrollTop = bodyRef.scrollHeight;
    }
  });

  function logLevel(line: string): Level {
    if (line.includes("ERROR") || line.includes("failed") || line.includes("Fail")) return "error";
    if (line.includes("done") || line.includes("Cache hit") || line.includes("Cache cleared") || line.includes("succeeded")) return "success";
    return "info";
  }

  const filtered = () => {
    const lvl = level();
    if (lvl === "all") return props.logs;
    return props.logs.filter((line) => logLevel(line) === lvl);
  };

  const count = (lvl: Level) => (lvl === "all" ? props.logs.length : props.logs.filter((l) => logLevel(l) === lvl).length);

  return (
    <>
      <div class="log-header">
        <span class="log-title">Event Log</span>
        <div class="log-filters">
          {(["all", "error", "success", "info"] as Level[]).map((lvl) => (
            <button
              class={`log-filter ${level() === lvl ? "active" : ""}`}
              data-level={lvl}
              onClick={() => setLevel(lvl)}
            >
              {lvl}
              <span class="log-count">{count(lvl)}</span>
            </button>
          ))}
        </div>
        <Show when={props.onClear}>
          <button class="log-clear" onClick={props.onClear} title="Clear log">
            Clear
          </button>
        </Show>
      </div>
      <div class="log-body" ref={bodyRef!}>
        <For each={filtered()}>
          {(line) => <div class={`log-line ${logLevel(line)}`}>{line}</div>}
        </For>
        <Show when={filtered().length === 0}>
          <div class="log-empty">No {level() === "all" ? "" : level() + " "}entries yet.</div>
        </Show>
      </div>
    </>
  );
}
