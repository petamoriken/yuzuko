const win = new Yuzuko.Window({
  title: "file-picker",
  width: 240,
  height: 120,
});
const opened = win.pickFiles({
  multiple: false,
  filters: [{ name: "Text", extensions: ["txt", "md"] }],
});
console.log(opened === null ? "open-cancel" : "open-picked");
const saved = win.pickFiles({
  mode: "save",
  suggestedName: "note.txt",
  filters: [{ name: "Text", extensions: ["txt"] }],
});
console.log(saved === null ? "save-cancel" : "save-picked");
win.close();
