const mode = Yuzuko.args[0];

function fail(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":ok");
  } catch (error) {
    const name = error instanceof Error ? error.name : "thrown";
    const message = error instanceof Error ? error.message : String(error);
    console.log(label + ":" + name + ":" + message);
  }
}

if (mode === "chdir") {
  console.log("before:" + Yuzuko.cwd());
  Yuzuko.chdir("nested");
  console.log("nested:" + Yuzuko.cwd());
  const note = await Yuzuko.readTextFile("note.txt");
  console.log("note:" + note);
  Yuzuko.chdir("..");
  console.log("back:" + Yuzuko.cwd());
  Yuzuko.chdir("");
  console.log("empty:" + Yuzuko.cwd());
  Yuzuko.chdir(".");
  console.log("dot:" + Yuzuko.cwd());
  fail("missing", () => {
    Yuzuko.chdir("missing-dir");
  });
  fail("file", () => {
    Yuzuko.chdir(new URL(import.meta.url));
  });
  fail("nul", () => {
    Yuzuko.chdir("has\0nul");
  });
  fail("https", () => {
    Yuzuko.chdir(new URL("https://example.invalid/dir"));
  });
  fail("type", () => {
    (Yuzuko.chdir as (directory: unknown) => void)(1);
  });
  Yuzuko.chdir(new URL("./", import.meta.url));
  console.log("ok:url");
} else {
  const get = Yuzuko.env.get as (
    name: string,
    extra?: string,
  ) => string | undefined;
  console.log("cwd:" + Yuzuko.cwd());
  console.log("probe:" + get("YUZUKO_PROCESS_PROBE", "ignored"));
  console.log(
    "missing:" + String(Yuzuko.env.get("YUZUKO_PROCESS_ABSENT_NAME")),
  );
  console.log(
    "lengths:" +
      [
        Yuzuko.env.get.length,
        Yuzuko.env.set.length,
        Yuzuko.env.delete.length,
        Yuzuko.env.has.length,
        Yuzuko.env.toObject.length,
        Yuzuko.cwd.length,
        Yuzuko.chdir.length,
        Yuzuko.exit.length,
      ].join(","),
  );
  Yuzuko.env.set("YuzukoEnvRoundTrip", "round");
  console.log("has:" + Yuzuko.env.has("YuzukoEnvRoundTrip"));
  console.log("get:" + Yuzuko.env.get("YuzukoEnvRoundTrip"));
  const snap = Yuzuko.env.toObject();
  Yuzuko.env.delete("YuzukoEnvRoundTrip");
  console.log("snap:" + snap.YuzukoEnvRoundTrip);
  console.log("after:" + String(Yuzuko.env.get("YuzukoEnvRoundTrip")));
  Yuzuko.env.set("YuzukoEnvEmpty", "");
  console.log("empty:" + JSON.stringify(Yuzuko.env.get("YuzukoEnvEmpty")));
  Yuzuko.env.delete("YuzukoEnvEmpty");
  fail("empty-name", () => {
    Yuzuko.env.get("");
  });
  fail("equals", () => {
    Yuzuko.env.get("A=B");
  });
  fail("nul-name", () => {
    Yuzuko.env.get("A\0B");
  });
  fail("nul-value", () => {
    Yuzuko.env.set("YuzukoEnvRoundTrip", "a\0b");
  });
  fail("name-type", () => {
    (Yuzuko.env.get as (name: unknown) => unknown)(1);
  });
  fail("value-type", () => {
    (Yuzuko.env.set as (name: string, value: unknown) => void)(
      "YuzukoEnvRoundTrip",
      1,
    );
  });
  Yuzuko.env.delete("YuzukoEnvMissingName");
  console.log("ok:delete-missing");
  Yuzuko.env.set("YuzukoCaseProbe", "CaseValue");
  console.log("case:" + String(Yuzuko.env.get("yuzukocaseprobe")));
  Yuzuko.env.delete("YuzukoCaseProbe");
  Yuzuko.env.delete("yuzukocaseprobe");
  Yuzuko.env.set("__proto__", "proto-value");
  const proto = Yuzuko.env.toObject();
  console.log(
    "proto-own:" + Object.prototype.hasOwnProperty.call(proto, "__proto__"),
  );
  console.log("proto-value:" + proto["__proto__"]);
  console.log(
    "proto-same:" + (Object.getPrototypeOf(proto) === Object.prototype),
  );
  Yuzuko.env.delete("__proto__");
  const worker = new Worker(
    new URL("./process_args_worker.js", import.meta.url),
  );
  const reply = await new Promise((resolve, reject) => {
    worker.onmessage = (event) => resolve(String(event.data));
    worker.onerror = () => reject(new Error("worker-error"));
    worker.postMessage("go");
  });
  worker.terminate();
  console.log("worker-args:" + reply);
}
