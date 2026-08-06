import { Show } from "solid-js";
import ArrowTableView from "./ArrowTableView";
import type { ExtractedRecord } from "../types";

interface Props {
  record: ExtractedRecord | null;
  extracting: boolean;
  onRestart: () => void;
  onBack: () => void;
}

export default function Step4_Extract(props: Props) {
  return (
    <div class="p-6 flex flex-col gap-4 h-full">
      <div class="page-heading">
        <div>
          <h2 class="text-xl font-semibold text-primary">Extract Data</h2>
          <p class="text-base text-muted mt-1">
            Run the compiled extraction logic on the document.
          </p>
        </div>
        <div class="page-actions">
          <Show when={props.extracting}>
            <span class="processing-indicator">
              <span class="processing-dot" />
              Extracting…
            </span>
          </Show>
        </div>
      </div>

      <Show
        when={props.record}
        fallback={
          <div class={`card ${props.extracting ? "loading" : "border-dashed"} text-muted text-base text-center p-8`}>
            {props.extracting ? "Extracting…" : "No data extracted yet. Complete previous steps first."}
          </div>
        }
      >
        <ArrowTableView record={props.record} />
      </Show>

      <div class="flex gap-2 mt-auto">
        <button
          class="btn btn-secondary"
          onClick={props.onBack}
          disabled={props.extracting}
        >
          Back
        </button>
        <button
          class="btn btn-secondary"
          onClick={props.onRestart}
          disabled={props.extracting}
        >
          Start Over
        </button>
      </div>
    </div>
  );
}
