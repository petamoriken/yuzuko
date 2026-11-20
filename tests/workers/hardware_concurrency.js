const languages = navigator.languages;

function assigned(name) {
  "use strict";
  try {
    navigator[name] = "x";
    return false;
  } catch (error) {
    return error instanceof TypeError;
  }
}

self.postMessage({
  hardwareConcurrency: navigator.hardwareConcurrency,
  language: navigator.language,
  languages,
  frozen: Object.isFrozen(languages),
  ownLanguage: Object.prototype.hasOwnProperty.call(navigator, "language"),
  appCodeName: navigator.appCodeName,
  appName: navigator.appName,
  appVersion: navigator.appVersion,
  product: navigator.product,
  onLine: navigator.onLine,
  userAgentReadonly: assigned("userAgent"),
  platformReadonly: assigned("platform"),
  coresReadonly: assigned("hardwareConcurrency"),
  appNameReadonly: assigned("appName"),
  gpuReadonly: assigned("gpu"),
});
