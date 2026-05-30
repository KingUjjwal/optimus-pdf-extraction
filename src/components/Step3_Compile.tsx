import { createSignal, Show } from "solid-js";

interface Props {
  layoutId: string;
  isCached: boolean;
  onCompile: () => Promise<void>;
  onNext: () => void;
  onBack: () => void;
  compiling: boolean;
  compileStatus: "idle" | "success" | "error";
  compileError?: string;
}

export default function Step3_Compile({ layoutId, isCached, onCompile, onNext, onBack, compiling, compileStatus, compileError }: Props) {
  return (
    <div class="p-6 flex flex-col gap-4 h-full">
      <div>
        <h2 class="text-xl font-semibold text-primary">
          Compile Extraction Logic
        </h2>
        <p class="mt-2 text-base text-muted">
          Generate and compile WebAssembly extraction code for this document layout.
        </p>
      </div>

      <div class="card">
        <div class="text-base text-muted mb-1">
          Layout ID
        </div>
        <div class="font-mono text-xs text-text">
          {layoutId || "Not determined"}
        </div>
        {isCached && (
          <div class="mt-2 text-xs text-success">
            ✓ Cached WASM available
          </div>
        )}
      </div>

      <Show when={!isCached}>
        <button
          class={`btn ${compileStatus === "error" ? "btn-danger" : "btn-secondary"}`}
          onClick={onCompile}
          disabled={compiling || compileStatus === "success"}
        >
          {compiling ? "Compiling..." : compileStatus === "success" ? "Compiled ✓" : "Compile"}
        </button>
      </Show>

      <Show when={compileError}>
        <div class="alert alert-danger">
          {compileError}
        </div>
      </Show>

      <div class="flex gap-2 mt-auto">
        <button
          class="btn btn-secondary"
          onClick={onBack}
          disabled={compiling}
        >
          Back
        </button>
        <button
          class="btn btn-primary"
          onClick={onNext}
          disabled={compiling || compileStatus === "error"}
        >
          Extract Data
        </button>
      </div>
    </div>
  );
}