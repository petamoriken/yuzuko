const omitted = new Yuzuko.Window();
const explicit = new Yuzuko.Window({ width: 10, height: 11 });
console.log(`${omitted.clientWidth}x${omitted.clientHeight}`);
console.log(`${explicit.clientWidth}x${explicit.clientHeight}`);
omitted.close();
explicit.close();
