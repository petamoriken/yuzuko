const first = new Yuzuko.Window({ width: 100, height: 80 });
const second = new Yuzuko.Window({ width: 90, height: 70 });
if (first.clientWidth !== 100 || second.clientHeight !== 70) {
  throw new Error("size");
}
second.close();
setTimeout(() => {
  first.close();
  console.log("both");
}, 0);
