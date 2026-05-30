import { createSignal, For } from "solid-js";

interface StepConfig {
  id: string;
  label: string;
  status: "pending" | "active" | "completed" | "error";
}

interface Props {
  steps: StepConfig[];
  activeStep: string;
  onStepClick: (stepId: string) => void;
}

export default function WizardStepper({ steps, activeStep, onStepClick }: Props) {
  const activeIndex = () => steps.findIndex(s => s.id === activeStep);

  return (
    <div class="flex items-center gap-0 p-4 bg-secondary border-b border">
      <For each={steps}>
        {(step, index) => (
          <>
            <div
              class={`flex items-center gap-2 ${step.status === "completed" || index() < activeIndex() ? "cursor-pointer" : ""} ${step.status === "pending" ? "opacity-50" : ""}`}
              onClick={() => {
                if (step.status === "completed" || index() < activeIndex()) {
                  onStepClick(step.id);
                }
              }}
            >
              <div
                class={`icon-circle-sm ${step.status === "active"
                  ? "bg-primary"
                  : step.status === "completed"
                  ? "bg-success"
                  : step.status === "error"
                  ? "bg-danger"
                  : "bg-tertiary"} border`}
              >
                {step.status === "completed" ? "✓" : index() + 1}
              </div>
              <span
                class={`text-base ${step.status === "active"
                  ? "text-text font-semibold"
                  : "text-muted"}`}
              >
                {step.label}
              </span>
            </div>
            {index() < steps.length - 1 && (
              <div
                class={`flex-1 h-1 bg-${step.status === "completed" ? "success" : "border"} mx-2`}
              />
            )}
          </>
        )}
      </For>
    </div>
  );
}