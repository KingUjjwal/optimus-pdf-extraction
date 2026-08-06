import { createSignal } from "solid-js";
import SchemaEditor from "./SchemaEditor";

interface Props {
  schema: string;
  onSchemaChange: (schema: string) => void;
  onInfer: (customPrompt?: string) => Promise<void>;
  onNext: () => void;
  onBack: () => void;
  inferring: boolean;
}

export default function Step2_Schema(props: Props) {
  return (
    <div class="p-6 flex flex-col gap-4 h-full">
      <div class="page-heading">
        <div>
          <h2 class="text-xl font-semibold text-primary">Define Extraction Schema</h2>
          <p class="text-base text-muted mt-1">
            Specify the fields to extract from the document. Use Infer Schema to auto-detect fields from the document structure.
          </p>
        </div>
      </div>

      <div class="card">
        <SchemaEditor
          schema={props.schema}
          onSchemaChange={props.onSchemaChange}
          onInfer={props.onInfer}
        />
      </div>

      <div class="flex gap-2 mt-auto">
        <button
          class="btn btn-secondary"
          onClick={props.onBack}
          disabled={props.inferring}
        >
          Back
        </button>
        <button
          class="btn btn-primary"
          onClick={props.onNext}
          disabled={props.inferring}
        >
          Generate Code
        </button>
      </div>
    </div>
  );
}
