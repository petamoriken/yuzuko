const win = new Yuzuko.Window({
  title: "Breakout",
  width: 640,
  height: 480,
});

const ctx = win.surface.getContext("2d")!;

const width = win.clientWidth;
const height = win.clientHeight;
const PADDLE_W = 96;
const PADDLE_H = 12;
const PADDLE_Y = height - 40;
const BALL_R = 8;
const BALL_SPEED = 420;
const COLUMNS = 10;
const ROWS = 6;
const COLORS = [
  "#e07a5f",
  "#f2cc8f",
  "#81b29a",
  "#7eb6d9",
  "#b5838d",
  "#e09f7d",
];

type Brick = { x: number; y: number; w: number; h: number; color: string };
type Mode = "serve" | "play" | "won" | "lost";

function makeBricks(): Brick[] {
  const gap = 6;
  const margin = 18;
  const top = 58;
  const w = (width - margin * 2 - gap * (COLUMNS - 1)) / COLUMNS;
  const h = 18;
  const bricks: Brick[] = [];
  for (let row = 0; row < ROWS; row++) {
    for (let column = 0; column < COLUMNS; column++) {
      bricks.push({
        x: margin + column * (w + gap),
        y: top + row * (h + gap),
        w,
        h,
        color: COLORS[row % COLORS.length],
      });
    }
  }
  return bricks;
}

let bricks = makeBricks();
let score = 0;
let lives = 3;
let mode: Mode = "serve";
let paddleX = width / 2;
const ball = { x: width / 2, y: PADDLE_Y - BALL_R - 1, vx: 0, vy: 0 };
const held = new Set<string>();

function clampPaddle(x: number): number {
  const half = PADDLE_W / 2;
  return Math.min(width - half, Math.max(half, x));
}

function stick(): void {
  ball.x = paddleX;
  ball.y = PADDLE_Y - BALL_R - 1;
  ball.vx = 0;
  ball.vy = 0;
}

function launch(): void {
  const angle = (Math.random() * 2 - 1) * (Math.PI / 5);
  ball.vx = Math.sin(angle) * BALL_SPEED;
  ball.vy = -Math.cos(angle) * BALL_SPEED;
  mode = "play";
}

function restart(): void {
  bricks = makeBricks();
  score = 0;
  lives = 3;
  mode = "serve";
  paddleX = width / 2;
  stick();
}

function loseLife(): void {
  lives -= 1;
  mode = lives > 0 ? "serve" : "lost";
  if (mode === "serve") {
    stick();
  }
}

function primary(): void {
  if (mode === "serve") {
    launch();
  } else if (mode === "won" || mode === "lost") {
    restart();
  }
}

function movePaddle(dt: number): void {
  let direction = 0;
  if (held.has("ArrowLeft") || held.has("KeyA")) {
    direction -= 1;
  }
  if (held.has("ArrowRight") || held.has("KeyD")) {
    direction += 1;
  }
  if (direction !== 0) {
    paddleX = clampPaddle(paddleX + direction * 560 * dt);
  }
}

function hits(brick: Brick): boolean {
  const nearestX = Math.max(brick.x, Math.min(ball.x, brick.x + brick.w));
  const nearestY = Math.max(brick.y, Math.min(ball.y, brick.y + brick.h));
  const dx = ball.x - nearestX;
  const dy = ball.y - nearestY;
  return dx * dx + dy * dy <= BALL_R * BALL_R;
}

function bounce(brick: Brick): void {
  const dx = ball.x - (brick.x + brick.w / 2);
  const dy = ball.y - (brick.y + brick.h / 2);
  const px = brick.w / 2 + BALL_R - Math.abs(dx);
  const py = brick.h / 2 + BALL_R - Math.abs(dy);
  if (px < py) {
    const sign = Math.sign(dx || 1);
    ball.vx = Math.abs(ball.vx) * sign;
    ball.x += sign * (px + 0.1);
  } else {
    const sign = Math.sign(dy || -1);
    ball.vy = Math.abs(ball.vy) * sign;
    ball.y += sign * (py + 0.1);
  }
}

function step(dt: number): void {
  ball.x += ball.vx * dt;
  ball.y += ball.vy * dt;
  if (ball.x < BALL_R) {
    ball.x = BALL_R;
    ball.vx = Math.abs(ball.vx);
  } else if (ball.x > width - BALL_R) {
    ball.x = width - BALL_R;
    ball.vx = -Math.abs(ball.vx);
  }
  if (ball.y < BALL_R) {
    ball.y = BALL_R;
    ball.vy = Math.abs(ball.vy);
  }
  if (ball.y - BALL_R > height) {
    loseLife();
    return;
  }
  const left = paddleX - PADDLE_W / 2;
  if (
    ball.vy > 0 &&
    ball.y + BALL_R >= PADDLE_Y &&
    ball.y - BALL_R <= PADDLE_Y + PADDLE_H &&
    ball.x >= left &&
    ball.x <= left + PADDLE_W
  ) {
    const hit = (ball.x - paddleX) / (PADDLE_W / 2);
    const speed = Math.hypot(ball.vx, ball.vy) || BALL_SPEED;
    const angle = Math.max(-1, Math.min(1, hit)) * (Math.PI / 3);
    ball.vx = Math.sin(angle) * speed;
    ball.vy = -Math.cos(angle) * speed;
    ball.y = PADDLE_Y - BALL_R - 0.1;
  }
  for (let i = bricks.length - 1; i >= 0; i--) {
    const brick = bricks[i];
    if (!hits(brick)) {
      continue;
    }
    bricks.splice(i, 1);
    score += 10;
    bounce(brick);
    if (bricks.length === 0) {
      mode = "won";
    }
    return;
  }
}

win.addEventListener("keydown", (event) => {
  if (event.code === "Escape") {
    win.close();
    return;
  }
  held.add(event.code);
  if (event.code === "Space" && !event.repeat) {
    primary();
  }
});
win.addEventListener("keyup", (event) => {
  held.delete(event.code);
});
win.addEventListener("blur", () => {
  held.clear();
});
win.addEventListener("pointermove", (event) => {
  paddleX = clampPaddle(event.offsetX);
});
win.addEventListener("pointerdown", (event) => {
  if (event.button === 0) {
    primary();
  }
});

stick();

let last = 0;
win.requestAnimationFrame(function frame(now: number) {
  win.requestAnimationFrame(frame);
  const dt = last === 0 ? 0 : Math.min(0.032, (now - last) / 1000);
  last = now;
  movePaddle(dt);
  if (mode === "serve") {
    stick();
  } else if (mode === "play") {
    let left = dt;
    while (left > 0 && mode === "play") {
      const slice = Math.min(left, 1 / 120);
      left -= slice;
      step(slice);
    }
  }

  ctx.reset();
  ctx.setTransform(win.devicePixelRatio, 0, 0, win.devicePixelRatio, 0, 0);
  ctx.fillStyle = "#12151c";
  ctx.fillRect(0, 0, width, height);
  for (const brick of bricks) {
    ctx.fillStyle = brick.color;
    ctx.fillRect(brick.x, brick.y, brick.w, brick.h);
  }
  ctx.fillStyle = "#f4f0e6";
  ctx.fillRect(paddleX - PADDLE_W / 2, PADDLE_Y, PADDLE_W, PADDLE_H);
  ctx.beginPath();
  ctx.arc(ball.x, ball.y, BALL_R, 0, Math.PI * 2);
  ctx.fill();

  ctx.font = "16px sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ctx.fillText(String(score), 18, 28);
  for (let i = 0; i < lives; i++) {
    ctx.beginPath();
    ctx.arc(width - 22 - i * 16, 28, 5, 0, Math.PI * 2);
    ctx.fill();
  }

  let message = "";
  let hint = "";
  if (mode === "serve") {
    message = "Click or Space to serve";
    hint = "Mouse or arrow keys";
  } else if (mode === "won") {
    message = "Cleared";
    hint = "Space to play again";
  } else if (mode === "lost") {
    message = "Out of balls";
    hint = "Space to play again";
  }
  if (message !== "") {
    ctx.textAlign = "center";
    ctx.font = "22px sans-serif";
    ctx.fillText(message, width / 2, height * 0.62);
    ctx.font = "16px sans-serif";
    ctx.fillStyle = "#9aa3b2";
    ctx.fillText(hint, width / 2, height * 0.62 + 28);
  }
});
