class IntlProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.port.postMessage({
      breakIterator: "v8BreakIterator" in Intl,
      yen: new Intl.NumberFormat("ja-JP", {
        style: "currency",
        currency: "JPY",
      }).format(1234),
      words: [...new Intl.Segmenter("ja", { granularity: "word" }).segment("東京都庁")].map(
        (part) => part.segment,
      ),
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      locale: Intl.DateTimeFormat().resolvedOptions().locale,
    });
  }

  process() {
    return false;
  }
}

registerProcessor("intl", IntlProcessor);
