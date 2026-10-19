const window = new Yuzuko.Window({
  title: "yuzuko",
  width: 320,
  height: 200,
});
console.log(`${window.clientWidth}x${window.clientHeight}`);
console.log(`${window.surface.width}x${window.surface.height}`);
console.log(
  `${Math.round(window.clientWidth * window.devicePixelRatio)}x${Math.round(window.clientHeight * window.devicePixelRatio)}`,
);
console.log(String(window.devicePixelRatio > 0));
window.moveTo(120, 140);
const frameX = window.outerX;
const frameY = window.outerY;
window.moveBy(30, -10);
console.log(`${window.outerX - frameX},${window.outerY - frameY}`);
console.log(String(window.screenX === window.innerX && window.screenY === window.innerY));
console.log(String(window.screenLeft === window.screenX && window.screenTop === window.screenY));
console.log(String(window.innerY + 0.5 >= window.outerY));
window.surface.title = "renamed";
console.log(window.surface.title);
window.hide();
console.log(String(window.visible));
window.show();
console.log(String(window.visible));
window.resizeBy(10, -20);
console.log(`${window.clientWidth}x${window.clientHeight}`);
const ctx = window.surface.getContext("2d");
if (ctx) {
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, window.surface.width, window.surface.height);
}
window.close();
console.log("window-closed");
