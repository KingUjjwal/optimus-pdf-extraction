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

export default function Step2_Schema({ schema, onSchemaChange, onInfer, onNext, onBack, inferring }: Props) {
  return (
    <div class="p-6 flex flex-col gap-4 h-full">
      <div>
        <h2 class="text-xl font-semibold text-primary">
          Define Extraction Schema
        </h2>
        <p class="mt-2 text-base text-muted">
          Specify the fields to extract from the document. Use Infer Schema to auto-detect fields from the document structure.
        </p>
      </div>

      <SchemaEditor
        schema={schema}
        onSchemaChange={onSchemaChange}
        onInfer={onInfer}
      />

      <div class="flex gap-2 mt-auto">
        <button
          class="btn btn-secondary"
          onClick={onBack}
          disabled={inferring}
        >
          Back
        </button>
        <button
          class="btn btn-primary"
          onClick={onNext}
          disabled={inferring}
        >
          Generate Code
        </button>
      </div>
    </div>
  );
}