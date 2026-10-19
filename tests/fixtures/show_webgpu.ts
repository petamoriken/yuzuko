import { drawBitmap, drawTriangle } from "../webgpu_draw.ts";

const left = new Yuzuko.Window({
  title: "gpu-left",
  width: 160,
  height: 120,
});
const right = new Yuzuko.Window({
  title: "gpu-right",
  width: 160,
  height: 120,
});

function finish(reason) {
  console.log(reason);
  left.close();
  right.close();
  console.log("window-closed");
}

const adapter = await navigator.gpu.requestAdapter();
if (!adapter) {
  finish("skipped");
} else {
  const device = await adapter.requestDevice();
  const format = navigator.gpu.getPreferredCanvasFormat();
  const leftContext = left.surface.getContext("webgpu");
  const rightContext = right.surface.getContext("webgpu");
  if (!leftContext || !rightContext) {
    finish("draw-failed missing context");
  } else {
    leftContext.configure({ device, format, alphaMode: "opaque" });
    rightContext.configure({ device, format, alphaMode: "premultiplied" });
    const image = new ImageData(
      new Uint8ClampedArray([0, 255, 0, 255]),
      1,
      1,
    );
    const bitmap = await createImageBitmap(image);
    let pending = 2;
    function done() {
      pending -= 1;
      if (pending === 0) {
        finish("presented");
      }
    }
    left.requestAnimationFrame(() => {
      try {
        drawTriangle(device, leftContext.getCurrentTexture(), format);
      } catch (error) {
        console.log(`draw-failed ${error}`);
      }
      done();
    });
    right.requestAnimationFrame(() => {
      try {
        drawBitmap(device, rightContext.getCurrentTexture(), format, bitmap);
      } catch (error) {
        console.log(`draw-failed ${error}`);
      }
      done();
    });
  }
}
