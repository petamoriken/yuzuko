self.onmessage = (event) => {
  const canvas = event.data;
  if (!(canvas instanceof OffscreenCanvas)) {
    self.postMessage({ kind: "not-canvas" });
    return;
  }
  const context = canvas.getContext("2d");
  const handle = requestAnimationFrame(() => {
    context.fillStyle = "#0000ff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    const pixel = context.getImageData(0, 0, 1, 1).data;
    self.postMessage({
      kind: "frame",
      handle,
      width: canvas.width,
      pixel: [pixel[0], pixel[1], pixel[2], pixel[3]],
    });
  });
};
