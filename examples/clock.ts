const win = new Yuzuko.Window({
  title: "Clock",
  width: 420,
  height: 480,
});

const ctx = win.surface.getContext("2d")!;

win.addEventListener("keydown", (event) => {
  if (event.code === "Escape") {
    win.close();
  }
});

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

function hand(
  cx: number,
  cy: number,
  angle: number,
  length: number,
  width: number,
  color: string,
): void {
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.beginPath();
  ctx.moveTo(cx, cy);
  ctx.lineTo(cx + Math.cos(angle) * length, cy + Math.sin(angle) * length);
  ctx.stroke();
}

win.requestAnimationFrame(function frame() {
  win.requestAnimationFrame(frame);
  const width = win.clientWidth;
  const height = win.clientHeight;
  const cx = width / 2;
  const cy = height / 2 - 24;
  const radius = Math.min(width, height) * 0.34;
  const date = new Date();
  const second =
    (date.getSeconds() / 60) * Math.PI * 2 - Math.PI / 2;
  const minute =
    ((date.getMinutes() + date.getSeconds() / 60) / 60) * Math.PI * 2 -
    Math.PI / 2;
  const hour =
    (((date.getHours() % 12) + date.getMinutes() / 60) / 12) * Math.PI * 2 -
    Math.PI / 2;

  ctx.reset();
  ctx.setTransform(win.devicePixelRatio, 0, 0, win.devicePixelRatio, 0, 0);
  ctx.fillStyle = "#141820";
  ctx.fillRect(0, 0, width, height);
  ctx.beginPath();
  ctx.arc(cx, cy, radius, 0, Math.PI * 2);
  ctx.fillStyle = "#1c212b";
  ctx.fill();
  ctx.lineCap = "round";
  for (let i = 0; i < 60; i++) {
    const angle = (i / 60) * Math.PI * 2 - Math.PI / 2;
    const major = i % 5 === 0;
    const inner = radius - (major ? 18 : 8);
    const outer = radius - 4;
    ctx.strokeStyle = major ? "#f4f0e6" : "#8b93a1";
    ctx.lineWidth = major ? 3 : 1;
    ctx.beginPath();
    ctx.moveTo(cx + Math.cos(angle) * inner, cy + Math.sin(angle) * inner);
    ctx.lineTo(cx + Math.cos(angle) * outer, cy + Math.sin(angle) * outer);
    ctx.stroke();
  }
  hand(cx, cy, hour, radius * 0.5, 6, "#f4f0e6");
  hand(cx, cy, minute, radius * 0.72, 4, "#f4f0e6");
  hand(cx, cy, second, radius * 0.78, 2, "#e07a5f");
  ctx.beginPath();
  ctx.arc(cx, cy, 5, 0, Math.PI * 2);
  ctx.fillStyle = "#e07a5f";
  ctx.fill();
  ctx.fillStyle = "#f4f0e6";
  ctx.font = "20px monospace";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const time = [
    pad(date.getHours()),
    pad(date.getMinutes()),
    pad(date.getSeconds()),
  ].join(":");
  ctx.fillText(time, cx, height - 42);
});
