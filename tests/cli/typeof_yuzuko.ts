console.log(typeof Yuzuko.test);
console.log(typeof Yuzuko.__test);
console.log(typeof Yuzuko.Window);
console.log(typeof requestAnimationFrame);

const build = Yuzuko.build;
console.log(typeof build);
console.log(build.target);
console.log(build.arch);
console.log(build.os);
console.log(build.vendor);
console.log(build.env === undefined ? "env-absent" : "env " + build.env);
console.log(build.standalone);
try {
  build.os = "freebsd";
  console.log("assigned");
} catch (error) {
  console.log(error instanceof TypeError ? "TypeError" : String(error));
}
console.log(build.os);

const worker = new Worker(
  "data:text/javascript," +
    encodeURIComponent(
      "postMessage(Yuzuko.build.target + ' ' + Yuzuko.build.os + ' ' + Yuzuko.build.standalone)",
    ),
);
try {
  const message = await new Promise((resolve, reject) => {
    worker.onmessage = (event) => resolve(event.data);
    worker.onerror = (event) => {
      reject(new Error(String(event.message || "worker error")));
    };
  });
  console.log("worker " + message);
} finally {
  worker.terminate();
}
