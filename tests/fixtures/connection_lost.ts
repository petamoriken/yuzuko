// The test cuts the Wayland connection after this window opens.
const win = new Yuzuko.Window({
  title: "connection-lost",
  width: 160,
  height: 100,
  active: false,
});
const ctx = win.surface.getContext("2d")!;
ctx.fillStyle = "teal";
ctx.fillRect(0, 0, 160, 100);
console.log("opened");
win.addEventListener("close", () => {
  console.log("close");
  setTimeout(() => {
    try {
      new Yuzuko.Window({ active: false });
      console.log("reopened");
    } catch (error) {
      const { name, message } = error as Error;
      console.log(`${name} ${message}`);
    }
    setTimeout(() => console.log("timer"), 50);
  }, 50);
});
