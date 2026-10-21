self.postMessage({
  breakIterator: "v8BreakIterator" in Intl,
  yen: new Intl.NumberFormat("ja-JP", {
    style: "currency",
    currency: "JPY",
  }).format(1234),
  words: [...new Intl.Segmenter("ja", { granularity: "word" }).segment("東京都庁")].map(
    (part) => part.segment,
  ),
  ar0: new Intl.PluralRules("ar").select(0),
  swedish: new Intl.Collator("sv").compare("ä", "z"),
  timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  locale: Intl.DateTimeFormat().resolvedOptions().locale,
});
