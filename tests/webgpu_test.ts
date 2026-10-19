import {
  channels,
  drawBitmap,
  drawTriangle,
  readPixel,
} from "./webgpu_draw.ts";

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function adapterOrSkip() {
  const fallback = await navigator.gpu.requestAdapter({
    forceFallbackAdapter: true,
  });
  assert(fallback === null, "fallback adapter");
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    console.log("webgpu-skipped");
    return null;
  }
  console.log("webgpu-ran");
  return adapter;
}

Yuzuko.test("preferred format and an empty feature request", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    let threw = false;
    try {
      navigator.gpu.getPreferredCanvasFormat();
    } catch (error) {
      threw = error.name === "InvalidStateError";
    }
    assert(threw, "preferred format without an adapter");
    return;
  }
  const format = navigator.gpu.getPreferredCanvasFormat();
  assert(
    format === "bgra8unorm" || format === "rgba8unorm",
    format,
  );
  assert(typeof adapter.features.has === "function", "features");
  assert(adapter.limits.maxTextureDimension2D >= 1, "limits");
  assert(adapter.isFallbackAdapter === false, "fallback flag");
  let rejected = false;
  try {
    await adapter.requestDevice({
      requiredFeatures: ["depth-clip-control"],
    });
  } catch (error) {
    rejected = error instanceof TypeError;
  }
  assert(rejected, "required feature");
  const device = await adapter.requestDevice();
  const above = device.limits.maxTextureDimension2D + 1;
  rejected = false;
  try {
    await adapter.requestDevice({
      requiredLimits: { maxTextureDimension2D: above },
    });
  } catch (error) {
    rejected = error instanceof TypeError;
  }
  assert(rejected, "required limit");
  device.destroy();
});

Yuzuko.test("a triangle readback and the same current texture", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const format = navigator.gpu.getPreferredCanvasFormat();
  const canvas = new OffscreenCanvas(16, 16);
  const context = canvas.getContext("webgpu");
  assert(context !== null, "context");
  context.configure({ device, format, alphaMode: "opaque" });
  const first = context.getCurrentTexture();
  const second = context.getCurrentTexture();
  assert(first === second, "same texture");
  drawTriangle(device, first, format);
  const center = channels(await readPixel(device, first, 8, 8), format);
  const corner = channels(await readPixel(device, first, 0, 0), format);
  assert(center.g > 200 && center.r < 40 && center.b < 40, "center green");
  assert(corner.r < 40 && corner.g < 40 && corner.b < 40, "corner clear");
  context.unconfigure();
  context.configure({ device, format, alphaMode: "premultiplied" });
  assert(context.getCurrentTexture() !== first, "new texture");
  device.destroy();
});

Yuzuko.test("an ImageBitmap is sampled as a texture", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const image = new ImageData(new Uint8ClampedArray([0, 255, 0, 255]), 1, 1);
  const bitmap = await createImageBitmap(image);
  const target = device.createTexture({
    size: [4, 4],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  drawBitmap(device, target, "rgba8unorm", bitmap);
  const pixel = channels(await readPixel(device, target, 0, 0), "rgba8unorm");
  assert(pixel.g > 200 && pixel.r < 40 && pixel.b < 40, "sampled green");
  device.destroy();
});

Yuzuko.test("two canvases keep distinct textures", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const format = navigator.gpu.getPreferredCanvasFormat();
  const left = new OffscreenCanvas(4, 4).getContext("webgpu");
  const right = new OffscreenCanvas(4, 4).getContext("webgpu");
  left.configure({ device, format });
  right.configure({ device, format });
  const leftTexture = left.getCurrentTexture();
  const rightTexture = right.getCurrentTexture();
  assert(leftTexture !== rightTexture, "distinct");
  clear(device, leftTexture, { r: 1, g: 0, b: 0, a: 1 });
  clear(device, rightTexture, { r: 0, g: 0, b: 1, a: 1 });
  const red = channels(await readPixel(device, leftTexture, 0, 0), format);
  const blue = channels(await readPixel(device, rightTexture, 0, 0), format);
  assert(red.r > 200 && red.b < 40, "left red");
  assert(blue.b > 200 && blue.r < 40, "right blue");
  device.destroy();
});

Yuzuko.test("validation scope, lost device, and unsupported compute", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  device.pushErrorScope("out-of-memory");
  assert(await device.popErrorScope() === null, "empty out-of-memory");
  device.pushErrorScope("validation");
  device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.VERTEX,
  });
  const error = await device.popErrorScope();
  assert(error instanceof GPUValidationError, "validation error");
  let unsupported = false;
  try {
    device.createComputePipeline();
  } catch (caught) {
    unsupported = caught.name === "NotSupportedError";
  }
  assert(unsupported, "compute pipeline");
  unsupported = false;
  try {
    device.createCommandEncoder().beginComputePass();
  } catch (caught) {
    unsupported = caught.name === "NotSupportedError";
  }
  assert(unsupported, "compute pass");
  const canvas = new OffscreenCanvas(4, 4);
  const context = canvas.getContext("webgpu");
  context.configure({
    device,
    format: navigator.gpu.getPreferredCanvasFormat(),
  });
  unsupported = false;
  try {
    device.createCommandEncoder().beginRenderPass({
      colorAttachments: [{
        view: context.getCurrentTexture().createView(),
        loadOp: "clear",
        storeOp: "store",
      }],
      depthStencilAttachment: {},
    });
  } catch (caught) {
    unsupported = caught.name === "NotSupportedError";
  }
  assert(unsupported, "depth attachment");
  const lost = device.lost;
  device.destroy();
  const info = await lost;
  assert(info.reason === "destroyed", "lost reason");
  let lostDevice = false;
  try {
    device.createBuffer({ size: 4, usage: GPUBufferUsage.COPY_DST });
  } catch (caught) {
    lostDevice = caught.name === "InvalidStateError";
  }
  assert(lostDevice, "device lost");
});

Yuzuko.test("writeBuffer round-trips through mapAsync", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const buffer = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  device.queue.writeBuffer(buffer, 0, new Uint8Array([1, 2, 3, 4]));
  await buffer.mapAsync(GPUMapMode.READ);
  const bytes = new Uint8Array(buffer.getMappedRange());
  assert(
    bytes[0] === 1 && bytes[1] === 2 && bytes[2] === 3 && bytes[3] === 4,
    "written bytes",
  );
  buffer.unmap();
  device.destroy();
});

function clear(device, texture, color) {
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: texture.createView(),
      clearValue: color,
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  pass.end();
  device.queue.submit([encoder.finish()]);
}
