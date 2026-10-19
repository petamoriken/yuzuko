const win = new Yuzuko.Window({
  title: "dialog-smoke",
  width: 240,
  height: 120,
});
const accepted = win.confirm("Continue?");
console.log(accepted ? "yes" : "no");
const name = win.prompt("Name:", "Ada");
console.log(name);
win.alert("done");
console.log("after");
win.close();
