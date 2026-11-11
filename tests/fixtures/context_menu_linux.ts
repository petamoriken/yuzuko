// Run by the Linux-only `context_menu` test with `yuzuko run`.
// showContextMenu needs a GTK window winit does not expose.

const win = new Yuzuko.Window({
  width: 80,
  height: 60,
  title: "context",
});
const menu = new Yuzuko.Menu([{ label: "Save", action: "ctx.save" }]);
win.setContextMenu(menu);
try {
  win.showContextMenu(menu, 1, 1);
  console.log("shown");
} catch (error) {
  console.log(error instanceof DOMException ? error.name : "");
  console.log(error instanceof Error ? error.message : "");
}
win.close();
