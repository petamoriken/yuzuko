const win = new Yuzuko.Window({
  title: "Paint",
  width: 800,
  height: 520,
});

const ctx = win.surface.getContext("2d")!;

const colors = [
  "#1c1917",
  "#e11d48",
  "#ea580c",
  "#ca8a04",
  "#15803d",
  "#2563eb",
  "#7c3aed",
];
const BAR = 56;
const SWATCH = 22;
const SWATCH_GAP = 10;

type Point = { x: number; y: number };
type Stroke = { color: string; points: Point[] };

const strokes: Stroke[] = [];
let color = colors[0];
let drawing = false;

function swatchX(index: number): number {
  return 16 + index * (SWATCH + SWATCH_GAP);
}

function swatchY(): number {
  return (BAR - SWATCH) / 2;
}

function colorAt(x: number, y: number): string | null {
  for (let i = 0; i < colors.length; i++) {
    const left = swatchX(i) - 4;
    const top = swatchY() - 4;
    if (
      x >= left &&
      x < left + SWATCH + 8 &&
      y >= top &&
      y < top + SWATCH + 8
    ) {
      return colors[i];
    }
  }
  return null;
}

win.addEventListener("keydown", (event) => {
  if (event.code === "Escape") {
    win.close();
    return;
  }
  if (event.code === "KeyC") {
    strokes.length = 0;
    drawing = false;
    return;
  }
  if (event.code.startsWith("Digit")) {
    const index = Number(event.code.slice(5)) - 1;
    if (index >= 0 && index < colors.length) {
      color = colors[index];
    }
  }
});
win.addEventListener("pointerdown", (event) => {
  if (event.button !== 0) {
    return;
  }
  if (event.offsetY <= BAR) {
    const next = colorAt(event.offsetX, event.offsetY);
    if (next !== null) {
      color = next;
    }
    drawing = false;
    return;
  }
  drawing = true;
  strokes.push({
    color,
    points: [{ x: event.offsetX, y: event.offsetY }],
  });
});
win.addEventListener("pointermove", (event) => {
  if (!drawing || (event.buttons & 1) === 0) {
    drawing = false;
    return;
  }
  const stroke = strokes[strokes.length - 1];
  const last = stroke.points[stroke.points.length - 1];
  const dx = event.offsetX - last.x;
  const dy = event.offsetY - last.y;
  if (dx * dx + dy * dy < 1) {
    return;
  }
  stroke.points.push({ x: event.offsetX, y: event.offsetY });
});
win.addEventListener("pointerup", (event) => {
  if (event.button === 0) {
    drawing = false;
  }
});
win.addEventListener("pointercancel", () => {
  drawing = false;
});

win.requestAnimationFrame(function frame() {
  win.requestAnimationFrame(frame);
  const width = win.clientWidth;
  const height = win.clientHeight;
  ctx.reset();
  ctx.setTransform(win.devicePixelRatio, 0, 0, win.devicePixelRatio, 0, 0);
  ctx.fillStyle = "#f6f1e7";
  ctx.fillRect(0, 0, width, height);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.lineWidth = 4;
  for (const stroke of strokes) {
    const points = stroke.points;
    if (points.length === 0) {
      continue;
    }
    ctx.fillStyle = stroke.color;
    ctx.strokeStyle = stroke.color;
    if (points.length === 1) {
      ctx.beginPath();
      ctx.arc(points[0].x, points[0].y, 2, 0, Math.PI * 2);
      ctx.fill();
      continue;
    }
    ctx.beginPath();
    ctx.moveTo(points[0].x, points[0].y);
    for (let i = 1; i < points.length; i++) {
      ctx.lineTo(points[i].x, points[i].y);
    }
    ctx.stroke();
  }

  ctx.fillStyle = "#ebe4d6";
  ctx.fillRect(0, 0, width, BAR);
  for (let i = 0; i < colors.length; i++) {
    const x = swatchX(i);
    const y = swatchY();
    ctx.fillStyle = colors[i];
    ctx.fillRect(x, y, SWATCH, SWATCH);
    if (colors[i] === color) {
      ctx.strokeStyle = "#1c1917";
      ctx.lineWidth = 2;
      ctx.strokeRect(x - 3, y - 3, SWATCH + 6, SWATCH + 6);
    }
  }
  ctx.fillStyle = "#6b645b";
  ctx.font = "14px sans-serif";
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText("1-7 color, C clears", swatchX(colors.length) + 8, BAR / 2);
});
