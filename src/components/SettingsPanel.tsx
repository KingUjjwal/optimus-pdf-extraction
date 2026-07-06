import { createSignal, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  cacheDir: string;
  addLog: (msg: string) => void;
}

export default function SettingsPanel({ cacheDir, addLog }: Props) {
  const [apiKey, setApiKey] = createSignal("");
  const [baseUrl, setBaseUrl] = createSignal("https://api.openai.com/v1");
  const [model, setModel] = createSignal("gpt-4o-mini");
  const [maxTokens, setMaxTokens] = createSignal(4096);
  const [saving, setSaving] = createSignal(false);
  const [saved, setSaved] = createSignal(false);

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    try {
      await invoke("save_llm_config_command", {
        config: {
          api_key: apiKey(),
          base_url: baseUrl(),
          model: model(),
          max_tokens: maxTokens(),
        },
        cacheDir,
      });
      setSaved(true);
      addLog("LLM config saved — restart app to apply");
    } catch (e) {
      addLog(`Save failed: ${e}`);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div class="p-6 flex flex-col gap-4">
      <div>
        <h2 class="text-xl font-semibold text-primary">Settings</h2>
        <p class="mt-2 text-base text-muted">
          Configure LLM provider for schema inference and code generation.
        </p>
      </div>

      <div class="card p-4 flex flex-col gap-3">
        <div>
          <label class="text-sm text-muted block mb-1">API Key</label>
          <input
            type="password"
            class="input"
            placeholder="sk-..."
            value={apiKey()}
            onInput={(e) => setApiKey(e.currentTarget.value)}
          />
        </div>

        <div>
          <label class="text-sm text-muted block mb-1">Base URL</label>
          <input
            type="text"
            class="input"
            value={baseUrl()}
            onInput={(e) => setBaseUrl(e.currentTarget.value)}
          />
        </div>

        <div>
          <label class="text-sm text-muted block mb-1">Model</label>
          <input
            type="text"
            class="input"
            value={model()}
            onInput={(e) => setModel(e.currentTarget.value)}
          />
        </div>

        <div>
          <label class="text-sm text-muted block mb-1">Max Tokens</label>
          <input
            type="number"
            class="input"
            value={maxTokens()}
            onInput={(e) => setMaxTokens(parseInt(e.currentTarget.value) || 4096)}
          />
        </div>

        <button
          class="btn btn-primary mt-2"
          onClick={handleSave}
          disabled={saving()}
        >
          {saving() ? "Saving..." : "Save Config"}
        </button>

        <Show when={saved()}>
          <div class="text-xs text-success mt-1">
            Saved. Restart the app for changes to take effect.
          </div>
        </Show>
      </div>
    </div>
  );
}
