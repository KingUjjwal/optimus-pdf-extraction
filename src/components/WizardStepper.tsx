import { For } from "solid-js";

interface StepConfig {
  id: string;
  label: string;
  hint?: string;
  status: "pending" | "active" | "completed" | "error";
}

interface Props {
  steps: StepConfig[];
  activeStep: string;
  onStepClick: (stepId: string) => void;
}

export default function WizardStepper(props: Props) {
  const activeIndex = () => props.steps.findIndex((s) => s.id === props.activeStep);

  return (
    <div class="stepper">
      <For each={props.steps}>
        {(step, index) => {
          const done = step.status === "completed";
          const errored = step.status === "error";
          const active = step.status === "active";
          const reachable = done || index() < activeIndex();
          const isLast = index() === props.steps.length - 1;

          return (
            <>
              <div
                class={`stepper-step ${step.status} ${reachable ? "clickable" : ""}`}
                onClick={() => {
                  if (reachable) props.onStepClick(step.id);
                }}
                role="button"
                aria-label={`${step.label}${done ? " (completed)" : ""}`}
                aria-current={active ? "step" : undefined}
              >
                <span class={`step-node ${errored ? "error" : done ? "done" : active ? "active" : ""}`}>
                  {done ? (
                    <svg viewBox="0 0 24 24" class="step-check" aria-hidden="true">
                      <path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
                    </svg>
                  ) : errored ? (
                    "!"
                  ) : (
                    index() + 1
                  )}
                </span>
                <span class="step-label">
                  {step.label}
                  {step.hint && <span class="step-hint">{step.hint}</span>}
                </span>
              </div>
              {!isLast && (
                <div class={`step-connector ${done ? "done" : ""}`}>
                  <span class="connector-fill" />
                </div>
              )}
            </>
          );
        }}
      </For>
    </div>
  );
}
