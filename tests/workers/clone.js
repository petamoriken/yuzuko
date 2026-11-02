self.onmessage = (event) => {
  const data = event.data;
  if (typeof ImageBitmap !== "undefined" && data instanceof ImageBitmap) {
    self.postMessage({
      kind: "bitmap",
      width: data.width,
      height: data.height,
    });
    return;
  }
  if (typeof ImageData !== "undefined" && data instanceof ImageData) {
    self.postMessage({
      kind: "image",
      width: data.width,
      height: data.height,
      colorSpace: data.colorSpace,
      pixelFormat: data.pixelFormat,
      pixel: data.data[0],
    });
    return;
  }
  if (typeof AudioData !== "undefined" && data instanceof AudioData) {
    const copy = new Float32Array(data.numberOfFrames);
    data.copyTo(copy, { planeIndex: 0 });
    self.postMessage({
      kind: "audio",
      frames: data.numberOfFrames,
      channels: data.numberOfChannels,
      rate: data.sampleRate,
      sample: copy[0],
    });
    data.close();
    return;
  }
  if (typeof VideoFrame !== "undefined" && data instanceof VideoFrame) {
    const canvas = new OffscreenCanvas(data.displayWidth, data.displayHeight);
    const context = canvas.getContext("2d");
    context.drawImage(data, 0, 0);
    const pixel = context.getImageData(0, 0, 1, 1).data;
    self.postMessage({
      kind: "frame",
      width: data.displayWidth,
      height: data.displayHeight,
      timestamp: data.timestamp,
      pixel: [pixel[0], pixel[1], pixel[2], pixel[3]],
    });
    data.close();
    return;
  }
  if (
    typeof EncodedAudioChunk !== "undefined"
    && data instanceof EncodedAudioChunk
  ) {
    self.postMessage({
      kind: "audio-chunk",
      type: data.type,
      timestamp: data.timestamp,
      byteLength: data.byteLength,
    });
    return;
  }
  if (
    typeof EncodedVideoChunk !== "undefined"
    && data instanceof EncodedVideoChunk
  ) {
    self.postMessage({
      kind: "video-chunk",
      type: data.type,
      timestamp: data.timestamp,
      byteLength: data.byteLength,
    });
    return;
  }
  self.postMessage({
    kind: "other",
    name: data && data.constructor ? data.constructor.name : typeof data,
  });
};
