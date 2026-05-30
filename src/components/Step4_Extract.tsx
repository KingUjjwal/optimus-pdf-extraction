import { Show } from "solid-js";
import ArrowTableView from "./ArrowTableView";
import type { ExtractedRecord } from "../types";

interface Props {
  record: ExtractedRecord | null;
  extracting: boolean;
  onRestart: () => void;
  onBack: () => void;
}

export default function Step4_Extract({ record, extracting, onRestart, onBack }: Props) {
  return (
    <div class="p-6 flex flex-col gap-4 h-full">
      <div>
        <h2 class="text-xl font-semibold text-primary">
          Extract Data
        </h2>
        <p class="mt-2 text-base text-muted">
          Run the compiled extraction logic on the document.
        </p>
      </div>

      <Show
        when={record}
        fallback={
          <div class="card border-dashed text-muted text-base text-center">
            {extracting ? "Extracting..." : "No data extracted yet. Complete previous steps first."}
          </div>
        }
      >
        <ArrowTableView record={record()} />
      </Show>

      <div class="flex gap-2 mt-auto">
        <button
          class="btn btn-secondary"
          onClick={onBack}
          disabled={extracting}
        >
          Back
        </button>
        <button
          class="btn btn-secondary"
          onClick={onRestart}
          disabled={extracting}
        >
          Start Over
        </button>
      </div>
    </div>
  );
}