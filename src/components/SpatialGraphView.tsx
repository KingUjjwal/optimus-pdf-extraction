import { createMemo, createSignal, createEffect, onMount, onCleanup } from "solid-js";
import type { TextSpan } from "../types";

interface Props {
  spans: TextSpan[];
}

interface Bounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

/** Compute bounds in a single pass — never `Math.min(...spread)`, which throws
 *  "Maximum call stack size exceeded" once a document has enough spans. */
function computeBounds(spans: TextSpan[]): Bounds {
  if (spans.length === 0) {
    return { minX: 0, maxX: 1, minY: 0, maxY: 1 };
  }
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const s of spans) {
    if (s.x0 < minX) minX = s.x0;
    if (s.x1 > maxX) maxX = s.x1;
    if (s.y0 < minY) minY = s.y0;
    if (s.y1 > maxY) maxY = s.y1;
  }
  return { minX, maxX, minY, maxY };
}

const PAD = 20;
const BIN_COUNT = 64;

export default function SpatialGraphView(props: Props) {
  let canvasRef!: HTMLCanvasElement;
  let containerRef!: HTMLDivElement;
  const [hovered, setHovered] = createSignal<number | null>(null);
  const [size, setSize] = createSignal({ w: 0, h: 0 });

  const bounds = createMemo(() => computeBounds(props.spans));

  /** Bucket span indices by document-space y-band so hit-testing scans only the
   *  rows near the cursor instead of every span. */
  const yBins = createMemo(() => {
    const b = bounds();
    const span = b.maxY - b.minY || 1;
    const bins: number[][] = Array.from({ length: BIN_COUNT }, () => []);
    const idx = (y: number) =>
      Math.min(BIN_COUNT - 1, Math.max(0, Math.floor(((y - b.minY) / span) * BIN_COUNT)));
    props.spans.forEach((s, i) => bins[idx((s.y0 + s.y1) / 2)].push(i));
    return bins;
  });

  function scaleFor(w: number, h: number) {
    const b = bounds();
    return {
      scaleX: (w - 2 * PAD) / (b.maxX - b.minX || 1),
      scaleY: (h - 2 * PAD) / (b.maxY - b.minY || 1),
    };
  }

  function draw() {
    const ctx = canvasRef.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvasRef.getBoundingClientRect();
    const w = rect.width;
    const h = rect.height;

    // Only reallocate the backing store when its size actually changed.
    // (Resetting width/height on every redraw — including each mousemove — was
    // reallocating the full canvas buffer continuously.)
    const bw = Math.round(w * dpr);
    const bh = Math.round(h * dpr);
    if (canvasRef.width !== bw || canvasRef.height !== bh) {
      canvasRef.width = bw;
      canvasRef.height = bh;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "hsl(226, 30%, 7%)";
    ctx.fillRect(0, 0, w, h);

    if (props.spans.length === 0) {
      ctx.fillStyle = "hsl(215,20%,65%)";
      ctx.fillText("No spans to render. Drop a PDF first.", PAD, PAD + 10);
      return;
    }

    const b = bounds();
    const { scaleX, scaleY } = scaleFor(w, h);
    const hov = hovered();

    for (let i = 0; i < props.spans.length; i++) {
      const s = props.spans[i];
      const sx = PAD + (s.x0 - b.minX) * scaleX;
      const sy = h - PAD - (s.y1 - b.minY) * scaleY;
      const sw = (s.x1 - s.x0) * scaleX;
      const sh = (s.y1 - s.y0) * scaleY;

      ctx.fillStyle = hov === i ? "rgba(0,242,254,0.3)" : "rgba(162,89,255,0.15)";
      ctx.fillRect(sx, sy, Math.max(sw, 4), Math.max(sh, 2));

      ctx.strokeStyle = hov === i ? "hsl(190,90%,50%)" : "hsla(217,33%,32%,0.5)";
      ctx.lineWidth = 0.5;
      ctx.strokeRect(sx, sy, Math.max(sw, 4), Math.max(sh, 2));

      if (sw > 30 || hov === i) {
        ctx.fillStyle = hov === i ? "#fff" : "hsl(215,20%,65%)";
        const fs = Math.min(11, Math.max(6, sh * 0.7));
        ctx.font = `${fs}px "Inter", system-ui`;
        ctx.fillText(s.text.slice(0, Math.floor(sw / 6)), sx + 2, sy + fs);
      }
    }
  }

  function hitTest(clientX: number, clientY: number): number | null {
    const b = bounds();
    const rect = canvasRef.getBoundingClientRect();
    const { scaleX, scaleY } = scaleFor(rect.width, rect.height);
    const mx = clientX - rect.left;
    const my = clientY - rect.top;

    // Convert mouse y to document space, then test only the nearby y-bins.
    const docY = b.minY + (rect.height - PAD - my) / (scaleY || 1);
    const spanY = b.maxY - b.minY || 1;
    const bin = Math.min(
      BIN_COUNT - 1,
      Math.max(0, Math.floor(((docY - b.minY) / spanY) * BIN_COUNT)),
    );
    const bins = yBins();
    const candidates: number[] = [];
    for (let k = bin - 1; k <= bin + 1; k++) {
      if (k >= 0 && k < BIN_COUNT) candidates.push(...bins[k]);
    }

    for (const i of candidates) {
      const s = props.spans[i];
      const sx = PAD + (s.x0 - b.minX) * scaleX;
      const sy = rect.height - PAD - (s.y1 - b.minY) * scaleY;
      const sw = (s.x1 - s.x0) * scaleX;
      const sh = (s.y1 - s.y0) * scaleY;
      if (mx >= sx && mx <= sx + sw + 4 && my >= sy && my <= sy + sh + 2) {
        return i;
      }
    }
    return null;
  }

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      const r = entries[0].contentRect;
      setSize({ w: r.width, h: r.height });
    });
    ro.observe(containerRef);
    onCleanup(() => ro.disconnect());
  });

  // Redraw when spans, hover, or container size change (coalesced by Solid).
  createEffect(() => {
    props.spans;
    hovered();
    size();
    draw();
  });

  return (
    <div
      ref={containerRef!}
      class="graph-canvas-container"
      onMouseMove={(e) => {
        const found = hitTest(e.clientX, e.clientY);
        if (found !== hovered()) setHovered(found);
      }}
      onMouseLeave={() => setHovered(null)}
    >
      <canvas ref={canvasRef!} class="graph-canvas" />
    </div>
  );
}
