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

const STATUS_LABEL: Record<Props["compileStatus"], string> = {
  idle: "Not started",
  compiling: "Compiling…",
  success: "Compiled",
  error: "Failed",
};

export default function Step3_Compile(props: Props) {
  return (
    <div class="p-6 flex flex-col gap-4 h-full">
      <div class="page-heading">
        <div>
          <h2 class="text-xl font-semibold text-primary">Compile Extraction Logic</h2>
          <p class="text-base text-muted mt-1">
            Generate and compile WebAssembly extraction code for this document layout.
          </p>
        </div>
        <div class="page-actions">
          <span class={`badge ${props.compileStatus === "success" ? "badge-success" : props.compileStatus === "error" ? "badge-danger" : props.compileStatus === "compiling" ? "badge-primary" : ""}`}>
            {STATUS_LABEL[props.compileStatus]}
          </span>
        </div>
      </div>

      <div class="card">
        <div class="text-base text-muted mb-1">
          Layout ID
        </div>
        <div class="font-mono text-xs text-text">
          {props.layoutId || "Not determined"}
        </div>
        {props.isCached && (
          <div class="mt-2 text-xs text-success">
            ✓ Cached WASM available
          </div>
        )}
      </div>

      <Show when={props.isCached} fallback={
        <>
          <button
            class={`btn ${props.compileStatus === "error" ? "btn-danger" : props.compileStatus === "success" ? "btn-success" : "btn-secondary"}`}
            onClick={props.onCompile}
            disabled={props.compiling || props.compileStatus === "success"}
          >
            {props.compiling ? "Compiling…" : props.compileStatus === "success" ? "Compiled ✓" : "Compile"}
          </button>

          <Show when={props.compileStatus === "error"}>
            <div class="alert alert-danger">
              <span class="alert-icon">⚠</span>
              <div class="flex-1 break-words">{props.compileError}</div>
            </div>
          </Show>
        </>
      }>
        <div class="card gradient-success">
          <div class="flex items-start gap-3">
            <div class="icon-circle-lg bg-success">✓</div>
            <div class="flex-1">
              <div class="font-semibold text-md text-text mb-1">Cached Format Available</div>
              <p class="text-base text-muted mb-3">
                This layout was previously compiled. Extraction will use the cached module and is instant.
              </p>
              <button
                class="btn btn-primary btn-block"
                onClick={props.onNext}
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
          onClick={props.onBack}
          disabled={props.compiling}
        >
          Back
        </button>
        <Show when={!props.isCached || props.compileStatus === "success"}>
          <button
            class="btn btn-primary"
            onClick={props.onNext}
            disabled={props.compiling || props.compileStatus === "error"}
          >
            Extract Data
          </button>
        </Show>
      </div>
    </div>
  );
}
