import { For, onMount, onCleanup, createSignal, createEffect } from "solid-js";
import type { TextSpan } from "../types";

interface Props {
  spans: TextSpan[];
}

export default function SpatialGraphView(props: Props) {
  let canvasRef!: HTMLCanvasElement;
  const [hovered, setHovered] = createSignal<number | null>(null);

  function draw() {
    const ctx = canvasRef.getContext("2d");
    if (!ctx || props.spans.length === 0) {
      ctx?.clearRect(0, 0, canvasRef.width, canvasRef.height);
      ctx?.fillText("No spans to render. Drop a PDF first.", 20, 30);
      return;
    }

    const dpr = window.devicePixelRatio || 1;
    const rect = canvasRef.getBoundingClientRect();
    canvasRef.width = rect.width * dpr;
    canvasRef.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const h = rect.height;

    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "hsl(222, 47%, 8%)";
    ctx.fillRect(0, 0, w, h);

    const minX = Math.min(...props.spans.map((s) => s.x0));
    const maxX = Math.max(...props.spans.map((s) => s.x1));
    const minY = Math.min(...props.spans.map((s) => s.y0));
    const maxY = Math.max(...props.spans.map((s) => s.y1));
    const scaleX = (w - 40) / (maxX - minX || 1);
    const scaleY = (h - 40) / (maxY - minY || 1);

    for (let i = 0; i < props.spans.length; i++) {
      const s = props.spans[i];
      const sx = 20 + (s.x0 - minX) * scaleX;
      const sy = h - 20 - (s.y1 - minY) * scaleY;
      const sw = (s.x1 - s.x0) * scaleX;
      const sh = (s.y1 - s.y0) * scaleY;

      ctx.fillStyle = hovered() === i ? "rgba(0,242,254,0.3)" : "rgba(162,89,255,0.15)";
      ctx.fillRect(sx, sy, Math.max(sw, 4), Math.max(sh, 2));

      ctx.strokeStyle = hovered() === i ? "hsl(190,90%,50%)" : "hsla(217,33%,32%,0.5)";
      ctx.lineWidth = 0.5;
      ctx.strokeRect(sx, sy, Math.max(sw, 4), Math.max(sh, 2));

      if (sw > 30 || hovered() === i) {
        ctx.fillStyle = hovered() === i ? "#fff" : "hsl(215,20%,65%)";
        const fs = Math.min(11, Math.max(6, sh * 0.7));
        ctx.font = `${fs}px "Inter", system-ui`;
        ctx.fillText(s.text.slice(0, Math.floor(sw / 6)), sx + 2, sy + fs);
      }
    }
  }

  createEffect(() => {
    const _ = props.spans;
    draw();
  });

  onMount(() => draw());

  return (
    <div
      class="graph-canvas-container"
      onMouseMove={(e) => {
        const rect = canvasRef.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;
        const minX = Math.min(...props.spans.map((s) => s.x0));
        const maxX = Math.max(...props.spans.map((s) => s.x1));
        const minY = Math.min(...props.spans.map((s) => s.y0));
        const maxY = Math.max(...props.spans.map((s) => s.y1));
        const scaleX = (rect.width - 40) / (maxX - minX || 1);
        const scaleY = (rect.height - 40) / (maxY - minY || 1);

        let found = null;
        for (let i = 0; i < props.spans.length; i++) {
          const s = props.spans[i];
          const sx = 20 + (s.x0 - minX) * scaleX;
          const sy = rect.height - 20 - (s.y1 - minY) * scaleY;
          const sw = (s.x1 - s.x0) * scaleX;
          const sh = (s.y1 - s.y0) * scaleY;
          if (mx >= sx && mx <= sx + sw + 4 && my >= sy && my <= sy + sh + 2) {
            found = i;
            break;
          }
        }
        setHovered(found);
        if (found !== null) draw();
      }}
      onMouseLeave={() => { setHovered(null); draw(); }}
    >
      <canvas ref={canvasRef!} class="graph-canvas" />
    </div>
  );
}
