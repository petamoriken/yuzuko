function report(kind, event) {
  const languages = navigator.languages;
  self.postMessage({
    kind,
    language: navigator.language,
    languages,
    onLine: navigator.onLine,
    trusted: event.isTrusted,
    bubbles: event.bubbles,
  });
}

self.addEventListener("languagechange", (event) => {
  report("languagechange", event);
});
self.addEventListener("online", (event) => {
  report("online", event);
});
self.addEventListener("offline", (event) => {
  report("offline", event);
});

const languages = navigator.languages;
self.postMessage({
  kind: "ready",
  language: navigator.language,
  languages,
  onLine: navigator.onLine,
  postMessageLength: postMessage.length,
  setter: typeof Yuzuko.__setHostNavigator,
});
