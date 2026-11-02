const mode = Yuzuko.args[0];

if (mode === "code") {
  Yuzuko.exit(Number(Yuzuko.args[1]), "no");
  console.log("after");
} else if (mode === "omit") {
  Yuzuko.exit();
  console.log("after");
} else if (mode === "negzero") {
  Yuzuko.exit(-0);
  console.log("after");
} else if (mode === "undef") {
  Yuzuko.exit(undefined);
  console.log("after");
} else if (mode === "catch") {
  try {
    Yuzuko.exit(4);
    console.log("still");
  } catch {
    console.log("caught");
  }
  console.log("after");
} else if (mode === "timer") {
  setInterval(() => {}, 1000);
  setTimeout(() => Yuzuko.exit(7), 20);
} else if (mode === "bad") {
  const samples = [
    1.5,
    -1,
    256,
    "1",
    NaN,
    Infinity,
    null,
    true,
    false,
    Object(1),
    new Number(1),
  ];
  for (const sample of samples) {
    try {
      Yuzuko.exit(sample as number);
      console.log("exited");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.log(message);
    }
  }
  console.log("stayed");
} else if (mode === "worker" || mode === "worker-module") {
  setInterval(() => {}, 1000);
  const url = new URL("./process_exit_worker.js", import.meta.url);
  const worker =
    mode === "worker-module"
      ? new Worker(url, { type: "module" })
      : new Worker(url);
  worker.onerror = () => {
    console.log("worker-error");
  };
  worker.onmessage = () => {
    console.log("worker-message");
  };
} else {
  console.log("unknown-mode");
}
