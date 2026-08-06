import { createSignal, Show, For, onMount, onCleanup } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { TextSpan } from "../types";

interface Props {
  onFileDrop: (path: string) => void;
  spans: TextSpan[];
  processing: boolean;
}

export default function PdfDropZone(props: Props) {
  const [dragOver, setDragOver] = createSignal(false);
  const [selectedPath, setSelectedPath] = createSignal<string | null>(null);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    // Tauri v2: HTML5 File objects don't carry a real path; the webview's
    // drag-drop event provides actual filesystem paths.
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "over" || payload.type === "enter") {
          setDragOver(true);
        } else if (payload.type === "drop") {
          setDragOver(false);
          const paths = payload.paths;
          if (paths && paths.length > 0) {
            setSelectedPath(paths[0]);
          }
        } else {
          setDragOver(false);
        }
      })
      .then((u) => {
        unlisten = u;
      });
    onCleanup(() => unlisten?.());
  });

  async function handleBrowse() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (selected) {
      setSelectedPath(selected as string);
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    setDragOver(true);
  }

  function handleDragLeave() {
    setDragOver(false);
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    setDragOver(false);
    const files = e.dataTransfer?.files;
    if (files && files.length > 0) {
      const path = (files[0] as unknown as { path?: string }).path || files[0].name;
      setSelectedPath(path);
    }
  }

  function handleSubmit() {
    const path = selectedPath();
    if (path) {
      props.onFileDrop(path);
    }
  }

  function handleClear() {
    setSelectedPath(null);
  }

  const fileName = () => {
    const p = selectedPath();
    return p ? p.split(/[/\\]/).pop() : null;
  };

  return (
    <div class="flex flex-col gap-4">
      <div class="page-heading">
        <div>
          <h2 class="text-xl font-semibold text-primary">Ingest Document</h2>
          <p class="text-base text-muted mt-1">
            Extract text spans, build the spatial graph, and run the extraction pipeline.
          </p>
        </div>
      </div>

      <div
        class={`drop-zone ${dragOver() ? "dragover" : ""}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={handleBrowse}
        role="button"
        aria-label="Drop a PDF here or click to browse"
      >
        <div class="drop-zone-icon">📄</div>
        <Show when={fileName()} fallback={<div class="drop-zone-text">Drop a PDF here or click to browse</div>}>
          <div class="drop-zone-text">{fileName()}</div>
          <div class="drop-zone-path">{selectedPath()}</div>
        </Show>
        <div class="drop-zone-cta">
          <button class="btn btn-primary" onClick={(e) => { e.stopPropagation(); handleBrowse(); }}>
            Browse…
          </button>
        </div>
      </div>

      <Show when={selectedPath()}>
        <div class="drop-zone-actions">
          <button
            class="btn btn-primary"
            onClick={handleSubmit}
            disabled={props.processing}
          >
            {props.processing ? "Processing…" : "Extract"}
          </button>
          <button
            class="btn btn-secondary"
            onClick={handleClear}
            disabled={props.processing}
          >
            Clear
          </button>
        </div>
      </Show>

      <Show when={props.spans.length > 0}>
        <div class="spans-preview">
          <strong>{props.spans.length}</strong> text spans extracted
          <div class="mt-2">
            <For each={props.spans.slice(0, 8)}>
              {(s) => <span class="chip">"{s.text.slice(0, 24)}"</span>}
            </For>
            {props.spans.length > 8 && <span class="chip">+{props.spans.length - 8} more…</span>}
          </div>
        </div>
      </Show>
    </div>
  );
}
