const names = Yuzuko.audioOutputDevices();
self.postMessage({
  fn: typeof Yuzuko.audioOutputDevices === "function",
  names,
});
