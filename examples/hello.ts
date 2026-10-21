const win = new Yuzuko.Window({
  title: "Hello, Yuzuko!",
  width: 720,
  height: 420,
});

const ctx = win.surface.getContext("2d")!;

win.addEventListener("keydown", (event) => {
  if (event.code === "Escape") {
    win.close();
  }
});

const text = "Hello, Yuzuko!";
const width = win.clientWidth;
const height = win.clientHeight;
ctx.setTransform(win.devicePixelRatio, 0, 0, win.devicePixelRatio, 0, 0);
ctx.fillStyle = "#141820";
ctx.fillRect(0, 0, width, height);
ctx.fillStyle = "#f4f0e6";
ctx.font = "64px sans-serif";
ctx.textAlign = "center";
ctx.textBaseline = "middle";
const measured = ctx.measureText(text).width;
ctx.fillText(text, width / 2, height / 2);
ctx.fillStyle = "#e07a5f";
ctx.fillRect(width / 2 - measured / 2, height / 2 + 46, measured, 3);
