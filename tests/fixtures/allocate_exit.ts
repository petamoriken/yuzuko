// Enough garbage for V8 to schedule its delayed memory reducer task. That
// task must not keep a finished script alive.
let keep: { n: number }[] = [];
const end = performance.now() + 500;
while (performance.now() < end) {
  keep.push({ n: keep.length });
  if (keep.length > 100_000) {
    keep = [];
  }
}
console.log("allocated");
