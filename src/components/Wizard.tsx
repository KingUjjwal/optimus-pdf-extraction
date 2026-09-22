import { createSignal, createMemo, Show, For } from "solid-js";
import type { TextSpan, ExtractedRecord, IngestFullResult } from "../types";
import WizardStepper from "./WizardStepper";
import Step0_Upload from "./Step0_Upload";
import Step2_Schema from "./Step2_Schema";
import Step3_Compile from "./Step3_Compile";
import Step4_Extract from "./Step4_Extract";
import { compileModuleLLM, extractCached, inferSchemaLLM } from "../lib/commands";

interface Props {
  schema: string;
  onSchemaChange: (schema: string) => void;
  latestSpans: () => TextSpan[];
  latestDocumentPath: () => string | undefined;
  latestRecord: () => ExtractedRecord | null;
  setLatestRecord: (record: ExtractedRecord) => void;
  layoutId: () => string;
  addLog: (msg: string) => void;
  checkCacheForLayout: (layoutId: string) => Promise<boolean>;
  cacheDir: string;
}

type StepStatus = "pending" | "active" | "completed" | "error";

function schemaSummary(schema: string): string {
  try {
    const parsed = JSON.parse(schema);
    const keys = Object.keys(parsed);
    return keys.length > 0 ? keys.join(", ") : "(empty schema)";
  } catch {
    return schema.slice(0, 60) || "(no schema)";
  }
}

export default function Wizard(props: Props) {
  const {
    onSchemaChange,
    latestSpans,
    latestDocumentPath,
    latestRecord,
    setLatestRecord,
    layoutId,
    addLog,
    checkCacheForLayout,
  } = props;
  const [activeStep, setActiveStep] = createSignal<"step0" | "step2" | "step3" | "step4">("step0");
  const [stepStatus, setStepStatus] = createSignal<Record<string, StepStatus>>({
    step0: "active",
    step2: "pending",
    step3: "pending",
    step4: "pending",
  });

  const [inferring, setInferring] = createSignal(false);
  const [compiling, setCompiling] = createSignal(false);
  const [extracting, setExtracting] = createSignal(false);
  const [compileError, setCompileError] = createSignal<string | undefined>();
  const [fieldConfidence, setFieldConfidence] = createSignal<Record<string, string>>({});
  const [isCached, setIsCached] = createSignal(false);

  const [ingestResult, setIngestResult] = createSignal<IngestFullResult | null>(null);

  function wizardSpans(): TextSpan[] {
    return ingestResult()?.spans || latestSpans();
  }

  function wizardLayoutId(): string {
    return ingestResult()?.layout_id || layoutId();
  }

  function wizardIsCached(): boolean {
    return ingestResult()?.is_cached ?? isCached();
  }

  function handleIngested(result: IngestFullResult) {
    setIngestResult(result);
    setIsCached(result.is_cached);
    setCompileError(undefined);
    updateStepStatus("step0", "completed");
    updateStepStatus("step2", "active");
    updateStepStatus("step3", "pending");
    updateStepStatus("step4", "pending");
    setActiveStep("step2");
  }

  const steps = [
    { id: "step0", label: "Upload", hint: "Select a PDF", status: () => stepStatus().step0 },
    { id: "step2", label: "Schema", hint: "Infer fields", status: () => stepStatus().step2 },
    { id: "step3", label: "Compile", hint: "Build WASM", status: () => stepStatus().step3 },
    { id: "step4", label: "Extract", hint: "Run extractor", status: () => stepStatus().step4 },
  ] as const;

  async function handleInfer(customPrompt?: string) {
    const spans = wizardSpans();
    if (spans.length === 0) {
      addLog("No spans loaded. Use the Upload step first.");
      throw new Error("No spans loaded — upload a PDF first");
    }

    const grid = ingestResult()?.grid || "";

    setInferring(true);
    try {
      const result = await inferSchemaLLM(grid, JSON.stringify(spans));
      onSchemaChange(result.schema);
      addLog(
        `Schema inferred (${result.provider_used}, ${result.fallback ? "fallback" : "LLM"}, ${result.token_usage.estimated_cost_cents} cents)`
      );

      updateStepStatus("step2", "completed");
      updateStepStatus("step3", "pending");
      updateStepStatus("step4", "pending");
    } catch (e) {
      addLog(`Schema inference failed: ${e}`);
      updateStepStatus("step2", "error");
      throw e;
    } finally {
      setInferring(false);
    }
  }

  async function handleCompile() {
    setCompiling(true);
    setCompileError(undefined);

    try {
      const lid = wizardLayoutId();
      if (!lid) {
        throw new Error("No layout ID available. Upload a PDF first.");
      }

      const spans = wizardSpans();
      if (spans.length === 0) {
        throw new Error("No spans available. Upload a PDF first.");
      }

      const result = await compileModuleLLM(
        lid,
        JSON.stringify(spans),
        props.schema,
        props.cacheDir,
        ingestResult()?.flat_graph
      );
      setIsCached(false);
      const fixInfo = result.llm_fix_attempts > 0 ? `, ${result.llm_fix_attempts} LLM fix(es)` : "";
      addLog(`Compiled ${result.size_bytes} bytes — ${result.compile_attempts} compile attempt(s), ${result.extraction_attempts} extraction attempt(s)${fixInfo} — ${result.token_usage.estimated_cost_cents} cents`);
      updateStepStatus("step3", "completed");
      updateStepStatus("step4", "pending");
    } catch (e) {
      const msg = String(e);
      setCompileError(msg);
      addLog(`Compile failed: ${msg}`);
      updateStepStatus("step3", "error");
    } finally {
      setCompiling(false);
    }
  }

  async function handleExtract(): Promise<boolean> {
    setExtracting(true);

    try {
      const lid = wizardLayoutId();
      if (!lid) {
        throw new Error("No layout ID available. Upload a PDF first.");
      }

      const result = await extractCached(lid, props.cacheDir);
      setLatestRecord(result.record);
      setFieldConfidence(result.field_confidence ?? {});
      addLog(`Extracted: ${JSON.stringify(result.record).slice(0, 80)}`);
      updateStepStatus("step4", "completed");
      return true;
    } catch (e) {
      addLog(`Extraction failed: ${e}`);
      updateStepStatus("step4", "error");
      return false;
    } finally {
      setExtracting(false);
    }
  }

  async function checkCacheForCurrentLayout() {
    const lid = wizardLayoutId();
    if (!lid) {
      setIsCached(false);
      return;
    }

    try {
      const cached = await checkCacheForLayout(lid);
      setIsCached(cached);
    } catch (_) {
      setIsCached(false);
    }
  }

  function updateStepStatus(stepId: string, status: StepStatus) {
    setStepStatus((prev) => {
      const updated = { ...prev, [stepId]: status };
      return updated;
    });
  }

  function handleStepClick(stepId: string) {
    const current = stepStatus();
    const currentIdx = steps.findIndex((s) => s.id === activeStep());
    const targetIdx = steps.findIndex((s) => s.id === stepId);

    if (targetIdx <= currentIdx || current[stepId] === "completed") {
      setActiveStep(stepId as "step0" | "step2" | "step3" | "step4");
    }
  }

  async function goToNextStep() {
    const current = activeStep();

    if (current === "step0") {
      updateStepStatus("step0", "completed");
      updateStepStatus("step2", "active");
      setActiveStep("step2");
    } else if (current === "step2") {
      updateStepStatus("step2", "completed");
      updateStepStatus("step3", "active");
      setActiveStep("step3");
      if (!ingestResult()) {
        await checkCacheForCurrentLayout();
      }
    } else if (current === "step3") {
      const ok = await handleExtract();
      if (ok) {
        setActiveStep("step4");
      }
    }
  }

  function goToPrevStep() {
    const current = activeStep();

    if (current === "step2") {
      updateStepStatus("step2", "pending");
      updateStepStatus("step0", "active");
      setActiveStep("step0");
    } else if (current === "step3") {
      updateStepStatus("step3", "pending");
      updateStepStatus("step2", "active");
      setActiveStep("step2");
    } else if (current === "step4") {
      updateStepStatus("step4", "pending");
      updateStepStatus("step3", "active");
      setActiveStep("step3");
    }
  }

  function restartWizard() {
    updateStepStatus("step0", "active");
    updateStepStatus("step2", "pending");
    updateStepStatus("step3", "pending");
    updateStepStatus("step4", "pending");
    setActiveStep("step0");
    setCompileError(undefined);
    setIngestResult(null);
    setIsCached(false);
  }

  return (
    <div class="flex flex-col h-full">
      <Show when={wizardSpans().length > 0 || wizardLayoutId() !== ""}>
        <div class="wizard-summary">
          <div class="wizard-summary-item">
            <span class="wizard-summary-label">Layout</span>
            <span class="wizard-summary-value">
              {wizardLayoutId() ? `${wizardLayoutId().slice(0, 24)}${wizardLayoutId().length > 24 ? "…" : ""}` : "—"}
            </span>
          </div>
          <div class="wizard-summary-item">
            <span class="wizard-summary-label">Spans</span>
            <span class="wizard-summary-value">{wizardSpans().length}</span>
          </div>
          <div class="wizard-summary-item">
            <span class="wizard-summary-label">Format</span>
            <span class={`wizard-summary-value ${wizardIsCached() ? "text-success" : ""}`}>
              {wizardIsCached() ? "cached" : "new"}
            </span>
          </div>
          <div class="wizard-summary-item">
            <span class="wizard-summary-label">Schema</span>
            <span class="wizard-summary-value muted">{schemaSummary(props.schema)}</span>
          </div>
        </div>
      </Show>

      <WizardStepper
        steps={steps.map((s) => ({ id: s.id, label: s.label, hint: s.hint, status: s.status() }))}
        activeStep={activeStep()}
        onStepClick={handleStepClick}
      />

      <div class="flex-1 overflow-auto">
        <Show when={activeStep() === "step0"}>
          <Step0_Upload
            cacheDir={props.cacheDir}
            onIngested={handleIngested}
            initialResult={ingestResult()}
          />
        </Show>

        <Show when={activeStep() === "step2"}>
          <Step2_Schema
            schema={props.schema}
            onSchemaChange={onSchemaChange}
            onInfer={handleInfer}
            onNext={goToNextStep}
            onBack={goToPrevStep}
            inferring={inferring()}
          />
        </Show>

        <Show when={activeStep() === "step3"}>
          <Step3_Compile
            layoutId={wizardLayoutId()}
            isCached={wizardIsCached()}
            onCompile={handleCompile}
            onNext={goToNextStep}
            onBack={goToPrevStep}
            compiling={compiling()}
            compileStatus={compiling() ? "compiling" : stepStatus().step3 === "completed" ? "success" : stepStatus().step3 === "error" ? "error" : "idle"}
            compileError={compileError()}
          />
        </Show>

        <Show when={activeStep() === "step4"}>
          <Step4_Extract
            record={latestRecord()}
            extracting={extracting()}
            onRestart={restartWizard}
            onBack={goToPrevStep}
            fieldConfidence={fieldConfidence()}
          />
        </Show>
      </div>
    </div>
  );
}