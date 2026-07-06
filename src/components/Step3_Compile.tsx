import { createSignal, Show } from "solid-js";

interface Props {
  layoutId: string;
  isCached: boolean;
  onCompile: () => Promise<void>;
  onNext: () => void;
  onBack: () => void;
  compiling: boolean;
  compileStatus: "idle" | "compiling" | "success" | "error";
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

      <Show when={isCached} fallback={
        <>
          <button
            class={`btn ${compileStatus === "error" ? "btn-danger" : "btn-secondary"}`}
            onClick={onCompile}
            disabled={compiling || compileStatus === "success"}
          >
            {compiling ? "Compiling..." : compileStatus === "success" ? "Compiled ✓" : "Compile"}
          </button>

          <Show when={compileError}>
            <div class="alert alert-danger">
              {compileError}
            </div>
          </Show>
        </>
      }>
        <div class="p-5 gradient-success rounded-lg">
          <div class="flex items-start gap-3">
            <div class="icon-circle-lg bg-success">✓</div>
            <div class="flex-1">
              <div class="font-semibold text-md text-text mb-1">Cached Format Available</div>
              <p class="text-base text-muted mb-3">
                This layout was previously compiled. Extraction will use the cached module and is instant.
              </p>
              <button
                class="btn btn-primary btn-block"
                onClick={onNext}
              >
                Extract with Cached Format →
              </button>
            </div>
          </div>
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
        <Show when={!isCached || compileStatus === "success"}>
          <button
            class="btn btn-primary"
            onClick={onNext}
            disabled={compiling || compileStatus === "error"}
          >
            Extract Data
          </button>
        </Show>
      </div>
    </div>
  );
}