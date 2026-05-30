import { createSignal, Show } from "solid-js";
import type { TextSpan } from "../types";

interface Props {
  spans: TextSpan[];
  documentPath?: string;
  onStart: () => void;
  disabled: boolean;
}

export default function Step0_Upload({ spans, documentPath, onStart, disabled }: Props) {
  const hasSpans = () => spans.length > 0;

  const getDocumentName = () => {
    if (!documentPath) return "Document";
    return documentPath.split(/[/\\]/).pop() || documentPath;
  };

  const getPreviewText = () => {
    if (spans.length === 0) return "";
    return spans.slice(0, 3).map(s => s.text.trim()).filter(t => t).join(" | ");
  };

  const getBoundingBox = () => {
    if (spans.length === 0) return { width: 0, height: 0 };
    const maxX = Math.max(...spans.map(s => s.x1));
    const maxY = Math.max(...spans.map(s => s.y1));
    return { width: Math.ceil(maxX), height: Math.ceil(maxY) };
  };

  const { width, height } = getBoundingBox();

  return (
    <div class="p-6 flex flex-col gap-4">
      <div>
        <h2 class="text-xl font-semibold text-primary">
          Upload Document
        </h2>
        <p class="mt-2 text-base text-muted">
          Drop a PDF file in the Ingest tab to extract text spans, or continue if a document is already loaded.
        </p>
      </div>

      <Show
        when={hasSpans()}
        fallback={
          <div class="p-8 bg-secondary border-dashed border-2 border-light rounded-lg text-muted text-base text-center">
            <div class="text-4xl mb-3 opacity-50">
              📄
            </div>
            <div class="mb-2">
              No document loaded yet
            </div>
            <div class="text-xs text-muted">
              Go to the <strong class="text-primary">Ingest</strong> tab and drop a PDF file to begin.
            </div>
          </div>
        }
      >
        <div class="p-5 gradient-success rounded-lg">
          <div class="flex items-start gap-3">
            <div class="icon-circle-lg bg-success">
              ✓
            </div>
            <div class="flex-1">
              <div class="font-semibold text-md text-text mb-1">
                Document Ready
              </div>
              <div class="text-base text-muted mb-2">
                {getDocumentName()}
              </div>

              <div class="stats-grid">
                <div class="stat-card">
                  <div class="stat-label">
                    Text Spans
                  </div>
                  <div class="stat-value">
                    {spans.length}
                  </div>
                </div>
                <div class="stat-card">
                  <div class="stat-label">
                    Pages
                  </div>
                  <div class="stat-value">
                    {Math.ceil(height / 800)}
                  </div>
                </div>
                <div class="stat-card">
                  <div class="stat-label">
                    Size (pts)
                  </div>
                  <div class="stat-value">
                    {width}x{height}
                  </div>
                </div>
              </div>

              {getPreviewText() && (
                <div class="mt-3 pt-3 border-t border-light">
                  <div class="stat-label">
                    Preview
                  </div>
                  <div class="text-xs text-text line-height-normal">
                    {getPreviewText()}...
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>

        <div class="flex gap-3 items-center mt-2">
          <button
            class="btn btn-primary btn-block"
            onClick={onStart}
            disabled={disabled}
          >
            Continue to Schema →
          </button>
          <Show when={disabled}>
            <span class="text-xs text-muted">
              Processing...
            </span>
          </Show>
        </div>
      </Show>
    </div>
  );
}