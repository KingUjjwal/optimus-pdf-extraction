import { createSignal, Show } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import type { TextSpan } from "../types";

interface Props {
  onFileDrop: (path: string) => void;
  spans: TextSpan[];
  processing: boolean;
}

export default function PdfDropZone({ onFileDrop, spans, processing }: Props) {
  const [dragOver, setDragOver] = createSignal(false);
  const [selectedPath, setSelectedPath] = createSignal<string | null>(null);

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
      onFileDrop(path);
    }
  }

  function handleClear() {
    setSelectedPath(null);
  }

  return (
    <div>
      <div
        class={`drop-zone ${dragOver() ? "dragover" : ""}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={handleBrowse}
      >
        <div class="drop-zone-icon">📄</div>
        <div class="drop-zone-text">Drop a PDF here or click to browse</div>
        <Show when={selectedPath()}>
          <div class="drop-zone-path">{selectedPath()}</div>
        </Show>
      </div>
      <Show when={selectedPath()}>
        <div class="drop-zone-actions">
          <button
            class="btn btn-primary"
            onClick={handleSubmit}
            disabled={processing}
          >
            {processing ? "Processing..." : "Extract"}
          </button>
          <button
            class="btn btn-secondary"
            onClick={handleClear}
            disabled={processing}
          >
            Clear
          </button>
        </div>
      </Show>
      {spans.length > 0 && (
        <div class="spans-bar">
          <strong>{spans.length}</strong> text spans extracted
          {spans.slice(0, 5).map((s) => (
            <span style={{ "margin-left": "8px", "font-family": "var(--font-mono)" }}>
              "{s.text.slice(0, 20)}"
            </span>
          ))}
          {spans.length > 5 && " ..."}
        </div>
      )}
    </div>
  );
}
