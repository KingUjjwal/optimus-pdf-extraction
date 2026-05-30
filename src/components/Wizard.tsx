import { createSignal, Show, For } from "solid-js";
import type { TextSpan, ExtractedRecord } from "../types";
import WizardStepper from "./WizardStepper";
import Step0_Upload from "./Step0_Upload";
import Step2_Schema from "./Step2_Schema";
import Step3_Compile from "./Step3_Compile";
import Step4_Extract from "./Step4_Extract";
import { discoverSchema, compileModuleLLM, extractCached } from "../lib/commands";

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

export default function Wizard({
  schema,
  onSchemaChange,
  latestSpans,
  latestDocumentPath,
  latestRecord,
  setLatestRecord,
  layoutId,
  addLog,
  checkCacheForLayout,
  cacheDir,
}: Props) {
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
  const [isCached, setIsCached] = createSignal(false);

  const steps = [
    { id: "step0", label: "Upload", status: () => stepStatus().step0 },
    { id: "step2", label: "Schema", status: () => stepStatus().step2 },
    { id: "step3", label: "Compile", status: () => stepStatus().step3 },
    { id: "step4", label: "Extract", status: () => stepStatus().step4 },
  ] as const;

  async function handleInfer(customPrompt?: string) {
    const spans = latestSpans();
    if (spans.length === 0) {
      addLog("No spans loaded. Drop a PDF first.");
      throw new Error("No spans loaded — process a PDF first");
    }

    setInferring(true);
    try {
      const result = await discoverSchema(JSON.stringify(spans), customPrompt);
      onSchemaChange(result);
      addLog("Schema inferred from document spans");

      updateStepStatus("step2", "completed");
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
      const lid = layoutId();
      if (!lid) {
        throw new Error("No layout ID available. Process a PDF first.");
      }

      const spans = latestSpans();
      if (spans.length === 0) {
        throw new Error("No spans available. Process a PDF first.");
      }

      const result = await compileModuleLLM(lid, JSON.stringify(spans), schema, cacheDir);
      setIsCached(false);
      addLog(`Compiled ${result.size_bytes} bytes in ${result.compile_attempts} attempt(s) — ${result.token_usage.estimated_cost_cents} cents`);
      updateStepStatus("step3", "completed");
    } catch (e) {
      const msg = String(e);
      setCompileError(msg);
      addLog(`Compile failed: ${msg}`);
      updateStepStatus("step3", "error");
    } finally {
      setCompiling(false);
    }
  }

  async function handleExtract() {
    setExtracting(true);

    try {
      const lid = layoutId();
      if (!lid) {
        throw new Error("No layout ID available. Process a PDF first.");
      }

      const result = await extractCached(lid, cacheDir);
      setLatestRecord(result.record);
      addLog(`Extracted: ${JSON.stringify(result.record.fields).slice(0, 80)}`);
      updateStepStatus("step4", "completed");
    } catch (e) {
      addLog(`Extraction failed: ${e}`);
      updateStepStatus("step4", "error");
    } finally {
      setExtracting(false);
    }
  }

  async function checkCacheForCurrentLayout() {
    const lid = layoutId();
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

  function handleStepClick(stepId: "step0" | "step2" | "step3" | "step4") {
    const current = stepStatus();
    const currentIdx = steps.findIndex((s) => s.id === activeStep());
    const targetIdx = steps.findIndex((s) => s.id === stepId);

    if (targetIdx <= currentIdx || current[stepId] === "completed") {
      setActiveStep(stepId);
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
      await checkCacheForCurrentLayout();
    } else if (current === "step3") {
      await handleExtract();
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
  }

  return (
    <div class="flex flex-col h-full">
      <WizardStepper
        steps={steps.map((s) => ({ id: s.id, label: s.label, status: s.status() }))}
        activeStep={activeStep()}
        onStepClick={handleStepClick}
      />

      <div class="flex-1 overflow-auto">
        <Show when={activeStep() === "step0"}>
          <Step0_Upload
            spans={latestSpans()}
            documentPath={latestDocumentPath()}
            onStart={goToNextStep}
            disabled={latestSpans().length === 0}
          />
        </Show>

        <Show when={activeStep() === "step2"}>
          <Step2_Schema
            schema={schema}
            onSchemaChange={onSchemaChange}
            onInfer={handleInfer}
            onNext={goToNextStep}
            onBack={goToPrevStep}
            inferring={inferring()}
          />
        </Show>

        <Show when={activeStep() === "step3"}>
          <Step3_Compile
            layoutId={layoutId()}
            isCached={isCached()}
            onCompile={handleCompile}
            onNext={goToNextStep}
            onBack={goToPrevStep}
            compiling={compiling()}
            compileStatus={compiling() ? "idle" : stepStatus().step3 === "completed" ? "success" : stepStatus().step3 === "error" ? "error" : "idle"}
            compileError={compileError()}
          />
        </Show>

        <Show when={activeStep() === "step4"}>
          <Step4_Extract
            record={latestRecord()}
            extracting={extracting()}
            onRestart={restartWizard}
            onBack={goToPrevStep}
          />
        </Show>
      </div>
    </div>
  );
}