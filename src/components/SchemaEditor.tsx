import { createSignal, createEffect, Show } from "solid-js";

interface Props {
  schema: string;
  onSchemaChange: (schema: string) => void;
  onInfer?: (customPrompt?: string) => Promise<void>;
}

export default function SchemaEditor(props: Props) {
  const [editing, setEditing] = createSignal(false);
  const [localSchema, setLocalSchema] = createSignal(props.schema);
  createEffect(() => {
    if (!editing()) setLocalSchema(props.schema);
  });
  const [error, setError] = createSignal<string | null>(null);
  const [customPrompt, setCustomPrompt] = createSignal<string>("");
  const [showPrompt, setShowPrompt] = createSignal(false);
  const [inferStatus, setInferStatus] = createSignal<string | null>(null);
  const [inferring, setInferring] = createSignal(false);

  function validate(s: string) {
    try {
      const parsed = JSON.parse(s);
      if (typeof parsed !== "object" || Array.isArray(parsed) || parsed === null) {
        setError("Schema must be a JSON object");
        return false;
      }
      setError(null);
      return true;
    } catch {
      setError("Invalid JSON");
      return false;
    }
  }

  function handleSave() {
    if (validate(localSchema())) {
      props.onSchemaChange(localSchema());
      setEditing(false);
    }
  }

  async function handleInfer() {
    if (!props.onInfer) return;
    setInferring(true);
    setInferStatus(null);
    try {
      await props.onInfer(customPrompt());
      setInferStatus("done");
    } catch (e) {
      setInferStatus("error");
      setError(String(e));
    } finally {
      setInferring(false);
      setTimeout(() => setInferStatus(null), 3000);
    }
  }

  return (
    <div style={{ "display": "flex", "flex-direction": "column", "gap": "8px" }}>
      <div style={{ "display": "flex", "align-items": "center", "gap": "8px" }}>
        <span style={{ "font-size": "13px", "color": "var(--text-muted)" }}>
          Extraction Schema
        </span>
        <button
          class="cache-clear-btn"
          style={{ "background": "var(--primary)", "margin": "0" }}
          onClick={() => setEditing(!editing())}
        >
          {editing() ? "Cancel" : "Edit"}
        </button>
        {props.onInfer && (
          <button
            class="cache-clear-btn"
            style={{ "background": "var(--secondary)", "margin": "0" }}
            onClick={handleInfer}
            disabled={inferring()}
          >
            {inferring() ? "Inferring..." : "Infer Schema"}
          </button>
        )}
        <button
          class="cache-clear-btn"
          style={{
            "background": "var(--bg-secondary)",
            "color": "var(--text)",
            "border": "1px solid var(--border)",
            "margin": "0",
          }}
          onClick={() => setShowPrompt(!showPrompt())}
        >
          {showPrompt() ? "Hide Prompt" : "Custom Prompt"}
        </button>
        <Show when={inferStatus() === "done"}>
          <span style={{ "color": "var(--success)", "font-size": "11px" }}>✓ Schema inferred</span>
        </Show>
        <Show when={inferStatus() === "error"}>
          <span style={{ "color": "var(--danger)", "font-size": "11px" }}>Inference failed</span>
        </Show>
      </div>

      <Show when={showPrompt()}>
        <textarea
          value={customPrompt()}
          onInput={(e) => setCustomPrompt(e.currentTarget.value)}
          placeholder="Optional: Add custom instructions for schema inference (e.g., 'Focus on financial fields: amount, date, invoice number')"
          rows={3}
          style={{
            "background": "var(--bg)",
            "color": "var(--text)",
            "border": "1px solid var(--border)",
            "border-radius": "var(--radius)",
            "padding": "8px",
            "font-family": "var(--font-mono)",
            "font-size": "12px",
            "resize": "vertical",
          }}
        />
      </Show>

      {editing() ? (
        <div style={{ "display": "flex", "flex-direction": "column", "gap": "4px" }}>
          <textarea
            value={localSchema()}
            onInput={(e) => setLocalSchema(e.currentTarget.value)}
            rows={6}
            style={{
              "background": "var(--bg)",
              "color": "var(--text)",
              "border": "1px solid var(--border)",
              "border-radius": "var(--radius)",
              "padding": "8px",
              "font-family": "var(--font-mono)",
              "font-size": "12px",
              "resize": "vertical",
            }}
          />
          {error() && (
            <span style={{ "color": "var(--danger)", "font-size": "11px" }}>{error()}</span>
          )}
          <button
            class="cache-clear-btn"
            style={{ "background": "var(--success)", "align-self": "flex-start" }}
            onClick={handleSave}
          >
            Save
          </button>
        </div>
      ) : (
        <pre
          style={{
            "background": "var(--bg)",
            "border": "1px solid var(--border)",
            "border-radius": "var(--radius)",
            "padding": "8px",
            "font-family": "var(--font-mono)",
            "font-size": "12px",
            "color": "var(--primary)",
            "white-space": "pre-wrap",
            "word-break": "break-all",
          }}
        >
          {props.schema || "No schema defined. Click Edit or Infer Schema."}
        </pre>
      )}
    </div>
  );
}
