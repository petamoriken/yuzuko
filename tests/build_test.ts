Yuzuko.test("build reports the host triple", () => {
  const build = Yuzuko.build;
  const names = Object.keys(build).join(",");
  if (names !== "target,arch,os,vendor,env,standalone") {
    throw new Error(names);
  }
  if (typeof build.target !== "string" || build.target.length === 0) {
    throw new Error("target");
  }
  const parts = build.target.split("-");
  if (build.arch !== parts[0]) {
    throw new Error("arch");
  }
  if (build.vendor !== parts[1]) {
    throw new Error("vendor");
  }
  if (build.os !== parts[2]) {
    throw new Error("os");
  }
  if (!["darwin", "linux", "windows"].includes(build.os)) {
    throw new Error("os value");
  }
  if (!["x86_64", "aarch64"].includes(build.arch)) {
    throw new Error("arch value");
  }
  const env = parts[3];
  if (env === undefined) {
    if (build.env !== undefined) {
      throw new Error("env present");
    }
  } else if (build.env !== env) {
    throw new Error("env");
  }
  if (!Object.prototype.hasOwnProperty.call(build, "env")) {
    throw new Error("env own");
  }
  if (build.standalone !== false) {
    throw new Error("standalone");
  }
  if (!Object.isFrozen(build)) {
    throw new Error("frozen");
  }
  const os = build.os;
  try {
    build.os = "freebsd";
    throw new Error("assigned");
  } catch (error) {
    if (!(error instanceof TypeError)) {
      throw error;
    }
  }
  if (build.os !== os) {
    throw new Error("os changed");
  }
});

Yuzuko.test("build is the same triple in a dedicated worker", async () => {
  const worker = new Worker(
    "data:text/javascript," +
      encodeURIComponent(
        [
          "const os = Yuzuko.build.os;",
          "Yuzuko.build.os = 'freebsd';",
          "postMessage([",
          "  Yuzuko.build.target,",
          "  Yuzuko.build.os,",
          "  Yuzuko.build.standalone,",
          "  typeof Yuzuko.test,",
          "  Yuzuko.build.os === os ? 'unchanged' : 'changed',",
          "].join(' '));",
          "close();",
        ].join("\n"),
      ),
  );
  try {
    const message = await new Promise((resolve, reject) => {
      worker.onmessage = (event) => resolve(event.data);
      worker.onerror = (event) => {
        reject(new Error(String(event.message || "worker error")));
      };
    });
    const expected = [
      Yuzuko.build.target,
      Yuzuko.build.os,
      String(Yuzuko.build.standalone),
      "undefined",
      "unchanged",
    ].join(" ");
    if (message !== expected) {
      throw new Error(String(message));
    }
  } finally {
    worker.terminate();
  }
});
