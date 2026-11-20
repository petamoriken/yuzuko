function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function positive(value: unknown, message: string): asserts value is number {
  assert(typeof value === "number", message);
  assert(Number.isInteger(value), `${message} integer`);
  assert(value >= 1, `${message} positive`);
}

Yuzuko.test("navigator.hardwareConcurrency is a positive integer", async () => {
  positive(navigator.hardwareConcurrency, "main");
  assert(typeof navigator.getGamepads === "function", "replaced navigator");
  assert(navigator.language.length > 0, "language");
  const languages = navigator.languages;
  const again = navigator.languages;
  assert(languages !== again, "new languages array");
  assert(Object.isFrozen(languages), "frozen languages");
  assert(languages[0] === navigator.language, "first language");
  assert(navigator.appCodeName === "Mozilla", "main appCodeName");
  assert(navigator.appName === "Netscape", "main appName");
  assert(navigator.appVersion === "5.0", "main appVersion");
  assert(navigator.product === "Gecko", "main product");
  assert(navigator.onLine === true, "main onLine");
  assert(
    (() => {
      try {
        (navigator as { appName: string }).appName = "x";
        return false;
      } catch (error) {
        return error instanceof TypeError;
      }
    })(),
    "main appName readonly",
  );
  assert(
    !Object.prototype.hasOwnProperty.call(navigator, "language"),
    "language is not an own property",
  );
  const getter = Object.getOwnPropertyDescriptor(
    Navigator.prototype,
    "language",
  )?.get;
  assert(typeof getter === "function", "language getter");
  const worker = new Worker(
    new URL("./workers/hardware_concurrency.js", import.meta.url).href,
  );
  const report = await new Promise<{
    hardwareConcurrency: number;
    language: string;
    languages: string[];
    frozen: boolean;
    ownLanguage: boolean;
    appCodeName: string;
    appName: string;
    appVersion: string;
    product: string;
    onLine: boolean;
    userAgentReadonly: boolean;
    platformReadonly: boolean;
    coresReadonly: boolean;
    appNameReadonly: boolean;
    gpuReadonly: boolean;
  }>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data);
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  worker.terminate();
  positive(report.hardwareConcurrency, "worker");
  assert(
    report.hardwareConcurrency === navigator.hardwareConcurrency,
    "same count",
  );
  assert(report.language === navigator.language, "same language");
  assert(
    JSON.stringify(report.languages) === JSON.stringify(languages),
    "same languages",
  );
  assert(report.frozen, "worker languages frozen");
  assert(!report.ownLanguage, "worker language is not an own property");
  assert(report.appCodeName === "Mozilla", report.appCodeName);
  assert(report.appName === "Netscape", report.appName);
  assert(report.appVersion === "5.0", report.appVersion);
  assert(report.product === "Gecko", report.product);
  assert(report.onLine === true, "onLine");
  assert(report.userAgentReadonly, "userAgent");
  assert(report.platformReadonly, "platform");
  assert(report.coresReadonly, "hardwareConcurrency");
  assert(report.appNameReadonly, "appName");
  assert(report.gpuReadonly, "gpu");
});
