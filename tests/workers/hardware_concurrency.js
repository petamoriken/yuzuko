const languages = navigator.languages;
self.postMessage({
  hardwareConcurrency: navigator.hardwareConcurrency,
  language: navigator.language,
  languages,
  frozen: Object.isFrozen(languages),
  ownLanguage: Object.prototype.hasOwnProperty.call(navigator, "language"),
});
