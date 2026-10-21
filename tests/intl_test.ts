function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

type IntlProbe = {
  breakIterator: boolean;
  yen: string;
  words: string[];
  ar0?: string;
  swedish?: number;
  timeZone: string;
  locale: string;
};

function assertProbe(probe: IntlProbe, label: string, host: { timeZone: string; locale: string }) {
  assert(probe.breakIterator === false, `${label} v8BreakIterator`);
  assert(probe.yen === "￥1,234", `${label} yen ${probe.yen}`);
  assert(probe.words.length === 2, `${label} words ${probe.words.join("/")}`);
  assert(probe.words[0] === "東京" && probe.words[1] === "都庁", `${label} segmentation`);
  assert(probe.timeZone === host.timeZone, `${label} time zone ${probe.timeZone}`);
  assert(probe.locale === host.locale, `${label} locale ${probe.locale}`);
}

Yuzuko.test("full ICU Intl", async () => {
  assert(!("v8BreakIterator" in Intl), "v8BreakIterator");
  const supported = Intl.DateTimeFormat.supportedLocalesOf([
    "ja",
    "zh-Hans",
    "ar",
    "th",
    "hi",
  ]);
  assert(supported.join(",") === "ja,zh-Hans,ar,th,hi", `locales ${supported.join(",")}`);
  const yen = new Intl.NumberFormat("ja-JP", {
    style: "currency",
    currency: "JPY",
  }).format(1234);
  assert(yen === "￥1,234", yen);
  const plurals = new Intl.PluralRules("ar");
  assert(plurals.select(0) === "zero", plurals.select(0));
  assert(plurals.select(2) === "two", plurals.select(2));
  assert(plurals.select(3) === "few", plurals.select(3));
  const words = [...new Intl.Segmenter("ja", { granularity: "word" }).segment("東京都庁")].map(
    (part) => part.segment,
  );
  assert(words.join("/") === "東京/都庁", words.join("/"));
  assert(new Intl.Collator("sv").compare("ä", "z") > 0, "swedish order");
  const calendars = Intl.supportedValuesOf("calendar");
  assert(calendars.includes("japanese"), "japanese calendar");
  assert(calendars.includes("islamic"), "islamic calendar");
  const region = new Intl.DisplayNames(["zh-Hans"], { type: "region" }).of("JP");
  assert(region === "日本", region);

  const host = Intl.DateTimeFormat().resolvedOptions();
  assert(host.timeZone.length > 0, "time zone");
  assert(host.locale.length > 0, "locale");

  const worker = new Worker(new URL("./workers/intl.js", import.meta.url).href);
  const workerProbe = await new Promise<IntlProbe>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => resolve(event.data as IntlProbe);
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  assertProbe(workerProbe, "worker", host);
  assert(workerProbe.ar0 === "zero", `worker plural ${workerProbe.ar0}`);
  assert((workerProbe.swedish ?? 0) > 0, "worker swedish order");
  worker.terminate();

  const context = new AudioContext();
  try {
    const script = new URL("./fixtures/audio/intl-processor.js", import.meta.url);
    await context.audioWorklet.addModule(script.href);
    const workletProbe = await new Promise<IntlProbe>((resolve) => {
      const node = new AudioWorkletNode(context, "intl");
      node.port.onmessage = (event) => resolve(event.data as IntlProbe);
    });
    assertProbe(workletProbe, "worklet", host);
  } finally {
    await context.close();
  }
});
