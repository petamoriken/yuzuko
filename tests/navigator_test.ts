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

interface HostNavigatorState {
  languages?: readonly string[] | null;
  onLine?: boolean | null;
}

function setHostNavigator(
  t: Yuzuko.TestContext,
  state: HostNavigatorState,
): void {
  (
    t as Yuzuko.TestContext & {
      setHostNavigator(state: HostNavigatorState): void;
    }
  ).setHostNavigator(state);
}

function stageHostNavigator(
  t: Yuzuko.TestContext,
  state: HostNavigatorState,
): void {
  (
    t as Yuzuko.TestContext & {
      stageHostNavigator(state: HostNavigatorState): void;
    }
  ).stageHostNavigator(state);
}

interface NavigatorNotice {
  kind: string;
  language: string;
  languages: string[];
  onLine: boolean;
  trusted: boolean;
  bubbles: boolean;
  postMessageLength?: number;
  setter?: string;
}

function workerQueue(worker: Worker): {
  take(): Promise<NavigatorNotice>;
} {
  const items: NavigatorNotice[] = [];
  const resolvers: Array<(value: NavigatorNotice) => void> = [];
  const rejecters: Array<(reason: Error) => void> = [];
  let failure: Error | undefined;
  worker.onmessage = (event: MessageEvent) => {
    const resolve = resolvers.shift();
    const data: NavigatorNotice = event.data;
    if (resolve) {
      resolve(data);
    } else {
      items.push(data);
    }
  };
  worker.onerror = (event: ErrorEvent) => {
    if (failure) {
      return;
    }
    failure = new Error(event.message || "worker error");
    while (rejecters.length > 0) {
      const reject = rejecters.shift();
      if (reject) {
        reject(failure);
      }
    }
  };
  return {
    take() {
      const item = items.shift();
      if (item) {
        return Promise.resolve(item);
      }
      if (failure) {
        return Promise.reject(failure);
      }
      return new Promise((resolve, reject) => {
        resolvers.push(resolve);
        rejecters.push(reject);
      });
    },
  };
}

function withTimeout<T>(
  promise: Promise<T>,
  message: string,
  ms = 2000,
): Promise<T> {
  let timer: number | undefined;
  const timeout = new Promise<T>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(message));
    }, ms);
  });
  return Promise.race([promise, timeout]).finally(() => {
    clearTimeout(timer);
  });
}

function turn(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => {
      resolve();
    };
    channel.port2.postMessage(null);
  });
}

function once(type: "languagechange" | "online" | "offline"): Promise<Event> {
  return new Promise((resolve) => {
    addEventListener(type, (event) => resolve(event), { once: true });
  });
}

Yuzuko.test(
  "navigator languagechange and online follow the host",
  async (t) => {
    assert(!("__setHostNavigator" in Yuzuko), "no host setter");
    assert(
      typeof (Yuzuko as { setHostNavigator?: unknown }).setHostNavigator ===
        "undefined",
      "setter stays off Yuzuko",
    );
    assert(
      typeof (Yuzuko as { stageHostNavigator?: unknown }).stageHostNavigator ===
        "undefined",
      "stage stays off Yuzuko",
    );
    const worker = new Worker(
      new URL("./workers/navigator_events.js", import.meta.url).href,
    );
    const inbox = workerQueue(worker);
    try {
      const ready = await withTimeout(inbox.take(), "worker ready timed out");
      assert(ready.kind === "ready", "ready");
      assert(ready.language === navigator.language, "worker language");
      assert(
        JSON.stringify(ready.languages) ===
          JSON.stringify(navigator.languages),
        "worker languages",
      );
      assert(ready.onLine === navigator.onLine, "worker onLine");
      assert(ready.postMessageLength === 1, "worker postMessage length");
      assert(ready.setter === "undefined", "worker has no setter");

      const next =
        navigator.languages[0] === "zu-ZZ"
          ? ["en-US"]
          : ["zu-ZZ", "en-US"];
      const pending = once("languagechange");
      const workerPending = inbox.take();
      setHostNavigator(t, { languages: next });
      const event = await withTimeout(pending, "languagechange timed out");
      const fromWorker = await withTimeout(
        workerPending,
        "worker languagechange timed out",
      );
      assert(event.type === "languagechange", "type");
      assert(event.isTrusted === true, "trusted");
      assert(event.bubbles === false, "bubbles");
      assert(navigator.language === next[0], "language");
      const reported = navigator.languages;
      assert(reported !== navigator.languages, "new languages array");
      assert(Object.isFrozen(reported), "frozen languages");
      assert(
        JSON.stringify(reported) === JSON.stringify(next),
        "languages",
      );
      assert(fromWorker.kind === "languagechange", "worker kind");
      assert(fromWorker.language === next[0], "worker language changed");
      assert(
        JSON.stringify(fromWorker.languages) === JSON.stringify(next),
        "worker languages changed",
      );
      assert(fromWorker.trusted === true, "worker trusted");
      assert(fromWorker.bubbles === false, "worker bubbles");
      console.log("languagechange after list change");

      let repeated = false;
      const mark = () => {
        repeated = true;
      };
      addEventListener("languagechange", mark);
      setHostNavigator(t, { languages: next });
      await turn();
      removeEventListener("languagechange", mark);
      assert(repeated === false, "languagechange repeated");

      const firstBit = !navigator.onLine;
      const firstKind = firstBit ? "online" : "offline";
      const first = once(firstKind);
      const firstWorker = inbox.take();
      setHostNavigator(t, { onLine: firstBit });
      const firstEvent = await withTimeout(first, firstKind + " timed out");
      const firstReport = await withTimeout(
        firstWorker,
        "worker " + firstKind + " timed out",
      );
      assert(firstEvent.isTrusted === true, firstKind + " trusted");
      assert(firstEvent.bubbles === false, firstKind + " bubbles");
      assert(navigator.onLine === firstBit, firstKind + " onLine");
      assert(firstReport.kind === firstKind, "worker " + firstKind);
      assert(firstReport.onLine === firstBit, "worker " + firstKind + " bit");
      console.log(firstKind + " onLine " + firstBit);

      const secondBit = !firstBit;
      const secondKind = secondBit ? "online" : "offline";
      const second = once(secondKind);
      const secondWorker = inbox.take();
      setHostNavigator(t, { onLine: secondBit });
      const secondEvent = await withTimeout(
        second,
        secondKind + " timed out",
      );
      const secondReport = await withTimeout(
        secondWorker,
        "worker " + secondKind + " timed out",
      );
      assert(secondEvent.isTrusted === true, secondKind + " trusted");
      assert(navigator.onLine === secondBit, secondKind + " onLine");
      assert(secondReport.kind === secondKind, "worker " + secondKind);
      assert(
        secondReport.onLine === secondBit,
        "worker " + secondKind + " bit",
      );
      console.log(secondKind + " onLine " + secondBit);
    } finally {
      try {
        setHostNavigator(t, { languages: null, onLine: null });
      } finally {
        worker.terminate();
      }
    }
  },
);

Yuzuko.test(
  "a parked realm samples a staged navigator change",
  async (t) => {
    assert(
      typeof (Yuzuko as { stageHostNavigator?: unknown }).stageHostNavigator ===
        "undefined",
      "stage stays off Yuzuko",
    );
    const worker = new Worker(
      new URL("./workers/navigator_events.js", import.meta.url).href,
    );
    const inbox = workerQueue(worker);
    try {
      const ready = await withTimeout(inbox.take(), "worker ready timed out");
      assert(ready.kind === "ready", "ready");
      const next =
        navigator.languages[0] === "zu-ZZ" ? ["en-US"] : ["zu-ZZ", "en-US"];
      let saw = false;
      const mark = () => {
        saw = true;
      };
      addEventListener("languagechange", mark);
      stageHostNavigator(t, { languages: next });
      assert(navigator.language === next[0], "getter sees the staged list");
      await turn();
      assert(saw === false, "staging does not publish");
      removeEventListener("languagechange", mark);

      const started = performance.now();
      const pending = once("languagechange");
      const workerPending = inbox.take();
      // The safety timer is longer than the sample interval. Without a
      // capped wait the loop sleeps until this timer, and the event is late.
      const event = await withTimeout(
        pending,
        "sample languagechange timed out",
        8000,
      );
      const elapsed = performance.now() - started;
      assert(elapsed < 3500, "sampled while parked: " + elapsed);
      assert(event.isTrusted === true, "trusted");
      assert(event.bubbles === false, "bubbles");
      assert(navigator.language === next[0], "language");
      const fromWorker = await withTimeout(
        workerPending,
        "worker sample languagechange timed out",
        8000,
      );
      assert(fromWorker.kind === "languagechange", "worker kind");
      assert(fromWorker.language === next[0], "worker language");
      assert(fromWorker.trusted === true, "worker trusted");
      console.log("languagechange after parked sample " + elapsed);
    } finally {
      try {
        setHostNavigator(t, { languages: null, onLine: null });
      } finally {
        worker.terminate();
      }
    }
  },
);
