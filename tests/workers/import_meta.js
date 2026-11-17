self.onmessage = () => {
  self.postMessage({
    url: import.meta.url,
    filename: import.meta.filename,
    dirname: import.meta.dirname,
    resolved: import.meta.resolve("./import_meta.js"),
    aliased: import.meta.resolve("./wake_lock.js"),
    length: import.meta.resolve.length,
    name: import.meta.resolve.name,
  });
};
