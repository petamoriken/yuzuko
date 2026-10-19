self.onmessage = async (event) => {
  const data = event.data;
  if (data.kind === "module") {
    self.postMessage({ kind: "module", n: data.n + 1 });
    return;
  }
  if (data.kind === "font") {
    const response = await fetch(data.url);
    const bytes = await response.arrayBuffer();
    const face = new FontFace("WorkerOnly", bytes);
    const loaded = await face.load();
    self.fonts.add(loaded);
    const canvas = new OffscreenCanvas(80, 40);
    const context = canvas.getContext("2d");
    context.font = "20px sans-serif";
    const generic = context.measureText("A").width;
    context.font = "20px WorkerOnly";
    const width = context.measureText("A").width;
    self.postMessage({
      kind: "font",
      same: self.fonts === globalThis.fonts,
      width,
      generic,
      status: loaded.status,
    });
    return;
  }
  if (data.kind === "gpu") {
    try {
      await navigator.gpu.requestAdapter();
      self.postMessage({ kind: "gpu", name: "resolved" });
    } catch (error) {
      self.postMessage({
        kind: "gpu",
        name: error instanceof DOMException ? error.name : "thrown",
      });
    }
  }
};
