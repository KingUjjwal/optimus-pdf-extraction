import { For, Show, createSignal } from "solid-js";
import type { ExtractedRecord } from "../types";

interface Props {
  record: ExtractedRecord | null;
}

interface Column {
  key: string;
  label: string;
  numeric: boolean;
}

const PREFERRED_COLUMNS: { label: string; aliases: string[]; numeric: boolean }[] = [
  { label: "Date", aliases: ["date", "txn_date", "transaction_date", "posted_date", "value_date", "dated"], numeric: false },
  { label: "Narration", aliases: ["narration", "description", "details", "remarks", "memo", "particulars", "note", "txn_narration"], numeric: false },
  { label: "Amount", aliases: ["amount", "value", "debit", "credit", "amount_inr", "amt", "txn_amount"], numeric: true },
  { label: "Units", aliases: ["units", "qty", "quantity", "count", "shares", "nav_units"], numeric: true },
  { label: "Price", aliases: ["price", "rate", "unit_price", "price_per_unit", "nav"], numeric: true },
  { label: "Balance", aliases: ["balance", "closing_balance", "running_balance", "bal", "balance_inr"], numeric: true },
];

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function isObjectArray(v: unknown): v is Record<string, unknown>[] {
  return Array.isArray(v) && v.length > 0 && v.every(isRecord);
}

function isPrimitive(v: unknown): boolean {
  return v === null || v === undefined || typeof v === "string" || typeof v === "number" || typeof v === "boolean";
}

function stringify(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  if (typeof v === "number") return Number.isFinite(v) ? String(v) : "";
  if (typeof v === "boolean") return String(v);
  return JSON.stringify(v);
}

function csvCell(v: unknown): string {
  return `"${stringify(v).replace(/"/g, '""')}"`;
}

function unwrapRecord(record: ExtractedRecord): ExtractedRecord {
  const keys = Object.keys(record);
  if (keys.length === 1 && keys[0] === "fields" && isRecord(record.fields)) {
    return record.fields;
  }
  return record;
}

function buildColumns(rows: Record<string, unknown>[]): Column[] {
  const present = new Set<string>();
  for (const r of rows) for (const k of Object.keys(r)) present.add(k);

  const columns: Column[] = [];
  for (const pref of PREFERRED_COLUMNS) {
    const match = pref.aliases.find((a) => present.has(a));
    if (match) columns.push({ key: match, label: pref.label, numeric: pref.numeric });
  }
  for (const k of present) {
    if (!columns.some((c) => c.key === k)) {
      columns.push({ key: k, label: k, numeric: false });
    }
  }
  return columns;
}

function formatCell(v: unknown, numeric: boolean): string {
  if (!numeric || typeof v !== "string" || v.trim() === "") return stringify(v);
  const n = Number(v.replace(/[^0-9.\-]/g, ""));
  if (Number.isNaN(n)) return v;
  return n.toLocaleString("en-US", { maximumFractionDigits: 2 });
}

function isNegative(v: unknown): boolean {
  if (typeof v === "number") return v < 0;
  if (typeof v === "string") {
    const n = Number(v.replace(/[^0-9.\-]/g, ""));
    return !Number.isNaN(n) && n < 0;
  }
  return false;
}

function prettyKey(key: string): string {
  return key
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

function buildCsv(record: ExtractedRecord): string {
  const entries = Object.entries(record);
  const scalar = entries.filter(([, v]) => !Array.isArray(v) && !isRecord(v));
  const objectArrays = entries.filter(([, v]) => isObjectArray(v)) as [string, Record<string, unknown>[]][];

  const parts: string[] = [];
  if (scalar.length > 0) {
    parts.push(scalar.map(([k]) => k).join(","));
    parts.push(scalar.map(([, v]) => csvCell(v)).join(","));
  }
  for (const [key, rows] of objectArrays) {
    const columns = buildColumns(rows);
    parts.push("");
    parts.push(key);
    parts.push(columns.map((c) => c.label).join(","));
    for (const r of rows) {
      parts.push(columns.map((c) => csvCell(r[c.key])).join(","));
    }
  }
  return parts.join("\n");
}

function tableCsv(rows: Record<string, unknown>[], columns: Column[]): string {
  const lines: string[] = [columns.map((c) => c.label).join(",")];
  for (const r of rows) {
    lines.push(columns.map((c) => csvCell(r[c.key])).join(","));
  }
  return lines.join("\n");
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand("copy");
      ta.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

function CopyButton(props: { label: string; getText: () => string }) {
  const [copied, setCopied] = createSignal(false);
  return (
    <button
      class={`copy-btn ${copied() ? "copied" : ""}`}
      onClick={async () => {
        const ok = await copyText(props.getText());
        if (ok) {
          setCopied(true);
          setTimeout(() => setCopied(false), 1600);
        }
      }}
    >
      {copied() ? "✓ Copied" : props.label}
    </button>
  );
}

function CollapsibleJson(props: { label: string; value: unknown }) {
  const [open, setOpen] = createSignal(false);
  return (
    <div class="collapse-block">
      <button class="collapse-toggle" onClick={() => setOpen((v) => !v)}>
        <span class={`collapse-caret ${open() ? "open" : ""}`}>▸</span>
        <span class="collapse-label">{props.label}</span>
        <span class="collapse-count">{Array.isArray(props.value) ? `${props.value.length} items` : "object"}</span>
      </button>
      <Show when={open()}>
        <pre class="collapse-pre">{JSON.stringify(props.value, null, 2)}</pre>
      </Show>
    </div>
  );
}

function PrimitiveList(props: { label: string; value: unknown[] }) {
  return (
    <div class="chip-list">
      <span class="chip-list-label">{prettyKey(props.label)}</span>
      <For each={props.value}>
        {(item) => <span class="chip">{stringify(item)}</span>}
      </For>
    </div>
  );
}

function CellValue(props: { value: unknown }) {
  if (isRecord(props.value)) {
    const count = Object.keys(props.value).length;
    return (
      <span class="cell-compound">
        <span class="cell-kind">obj</span>
        <span class="cell-snippet">{count} field{count === 1 ? "" : "s"}</span>
      </span>
    );
  }
  if (Array.isArray(props.value)) {
    return (
      <span class="cell-compound">
        <span class="cell-kind">{props.value.length} items</span>
        <span class="cell-snippet">array</span>
      </span>
    );
  }
  return <>{stringify(props.value)}</>;
}

export default function ArrowTableView({ record }: Props) {
  if (!record) {
    return (
      <div class="section-card empty">
        <p>No extraction results yet. Run the pipeline or open a document.</p>
      </div>
    );
  }

  const data = unwrapRecord(record);
  const entries = Object.entries(data);
  const scalar = entries.filter(([, v]) => !Array.isArray(v) && !isRecord(v));
  const objects = entries.filter(([, v]) => isRecord(v)) as [string, Record<string, unknown>][];
  const objectArrays = entries.filter(([, v]) => isObjectArray(v)) as [string, Record<string, unknown>[]][];
  const primitiveArrays = entries.filter(([, v]) => Array.isArray(v) && !isObjectArray(v)) as [string, unknown[]][];

  const transactionCount = () => objectArrays.reduce((sum, [, v]) => sum + v.length, 0);

  return (
    <div class="result-view">
      <div class="result-title-row">
        <h2 class="result-title">Extraction Results</h2>
        <div class="result-summary">
          <span class="summary-badge">{scalar.length} fields</span>
          <span class="summary-badge">{entries.length - scalar.length} structured</span>
          <Show when={transactionCount() > 0}>
            <span class="summary-badge highlight">{transactionCount()} transactions</span>
          </Show>
        </div>
        <div class="result-actions">
          <CopyButton label="Copy JSON" getText={() => JSON.stringify(data, null, 2)} />
          <CopyButton label="Copy CSV" getText={() => buildCsv(data)} />
        </div>
      </div>

      <Show when={scalar.length > 0} fallback={
        <div class="section-card empty">
          <p>No scalar fields extracted. The record contains only structured data (see below).</p>
        </div>
      }>
        <section class="section-card">
          <div class="section-head">
            <h3 class="section-title">Client Details</h3>
            <span class="section-meta">{scalar.length} fields</span>
          </div>
          <div class="kv-grid">
            <For each={scalar}>
              {([key, val]) => (
                <div class="kv-card" title={key}>
                  <div class="kv-label">{prettyKey(key)}</div>
                  <div class="kv-value">
                    <Show when={stringify(val) !== ""} fallback={<span class="kv-empty">—</span>}>
                      {stringify(val)}
                    </Show>
                  </div>
                </div>
              )}
            </For>
          </div>
        </section>
      </Show>

      <For each={objectArrays}>
        {([key, rows]) => {
          const columns = buildColumns(rows);
          return (
            <section class="txn-section">
              <div class="section-head">
                <h3 class="section-title">
                  {prettyKey(key)}
                  <span class="txn-count-badge">{rows.length} row{rows.length === 1 ? "" : "s"}</span>
                </h3>
                <div class="section-head-actions">
                  <CopyButton label="Copy table" getText={() => tableCsv(rows, columns)} />
                </div>
              </div>
              <TransactionsTable rows={rows} columns={columns} />
            </section>
          );
        }}
      </For>

      <Show when={objects.length > 0}>
        <section class="section-card">
          <div class="section-head">
            <h3 class="section-title">Structured Fields</h3>
            <span class="section-meta">{objects.length}</span>
          </div>
          <div class="collapse-stack">
            <For each={objects}>
              {([key, val]) => (
                <CollapsibleJson label={prettyKey(key)} value={val} />
              )}
            </For>
          </div>
        </section>
      </Show>

      <Show when={primitiveArrays.length > 0}>
        <section class="section-card">
          <div class="section-head">
            <h3 class="section-title">Lists</h3>
          </div>
          <For each={primitiveArrays}>
            {([key, val]) => <PrimitiveList label={key} value={val} />}
          </For>
        </section>
      </Show>

      <Show when={entries.length === 0}>
        <div class="section-card empty">
          <p>Extraction returned an empty record.</p>
        </div>
      </Show>
    </div>
  );
}

function TransactionsTable(props: { rows: Record<string, unknown>[]; columns: Column[] }) {
  return (
    <div class="table-scroll">
      <table class="txn-table">
        <thead>
          <tr>
            <For each={props.columns}>
              {(col) => (
                <th class={col.numeric ? "num" : ""}>{col.label}</th>
              )}
            </For>
          </tr>
        </thead>
        <tbody>
          <For each={props.rows}>
            {(row) => (
              <tr>
                <For each={props.columns}>
                  {(col) => {
                    const val = row[col.key];
                    const neg = isNegative(val);
                    const primitive = isPrimitive(val);
                    return (
                      <td class={col.numeric ? "num" : ""} data-neg={neg || undefined}>
                        {primitive ? formatCell(val, col.numeric) : <CellValue value={val} />}
                      </td>
                    );
                  }}
                </For>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
}
