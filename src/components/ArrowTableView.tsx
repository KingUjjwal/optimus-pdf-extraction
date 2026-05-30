import type { ExtractedRecord } from "../types";

interface Props {
  record: ExtractedRecord | null;
}

function toCsv(record: ExtractedRecord): string {
  const entries = Object.entries(record);
  const header = entries.map(([k]) => k).join(",");
  const row = entries.map(([, v]) => `"${v.replace(/"/g, '""')}"`).join(",");
  return `${header}\n${row}`;
}

export default function ArrowTableView({ record }: Props) {
  if (!record) {
    return <div class="no-data">No extraction results yet. Process a PDF.</div>;
  }

  const entries = Object.entries(record);

  return (
    <div>
      <div class="table-toolbar">
        <button
          class="table-btn"
          onClick={() => {
            navigator.clipboard.writeText(JSON.stringify(record, null, 2));
          }}
        >
          Copy JSON
        </button>
        <button
          class="table-btn"
          onClick={() => {
            navigator.clipboard.writeText(toCsv(record));
          }}
        >
          Copy CSV
        </button>
      </div>
      <table class="data-table">
        <thead>
          <tr>
            <th>Field</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          {entries.map(([key, val]) => (
            <tr><td>{key}</td><td class="mono">{val}</td></tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
