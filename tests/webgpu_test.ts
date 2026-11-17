import {
  channels,
  drawBitmap,
  drawTriangle,
  readPixel,
  triangleShader,
} from "./webgpu_draw.ts";

function assert(condition: unknown, message: string): void {
  if (!condition) {
    throw new Error(message);
  }
}

function errorName(error: unknown): string {
  return error instanceof Error ? error.name : "";
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
      threw = errorName(error) === "InvalidStateError";
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
  if (adapter.features.has("depth-clip-control")) {
    const requested = await adapter.requestDevice({
      requiredFeatures: ["depth-clip-control"],
    });
    assert(requested.features.has("depth-clip-control"), "depth clip");
    requested.destroy();
  } else {
    let rejected = false;
    try {
      await adapter.requestDevice({
        requiredFeatures: ["depth-clip-control"],
      });
    } catch (error) {
      rejected = error instanceof TypeError;
    }
    assert(rejected, "required feature");
  }
  let rejected = false;
  try {
    await adapter.requestDevice({
      // @ts-expect-error not-a-feature is not a GPUFeatureName.
      requiredFeatures: ["not-a-feature"],
    });
  } catch (error) {
    rejected = error instanceof TypeError;
  }
  assert(rejected, "invalid feature name");
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
  const context = canvas.getContext("webgpu")!;
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
  const left = new OffscreenCanvas(4, 4).getContext("webgpu")!;
  const right = new OffscreenCanvas(4, 4).getContext("webgpu")!;
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

Yuzuko.test("validation scope, lost device, and pass validation", async () => {
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
  let invalid = false;
  try {
    // @ts-expect-error createComputePipeline requires a descriptor.
    device.createComputePipeline.call(device);
  } catch (caught) {
    invalid = caught instanceof TypeError;
  }
  assert(invalid, "compute pipeline descriptor");
  const compute = device.createCommandEncoder().beginComputePass();
  compute.end();
  const canvas = new OffscreenCanvas(4, 4);
  const context = canvas.getContext("webgpu")!;
  context.configure({
    device,
    format: navigator.gpu.getPreferredCanvasFormat(),
  });
  invalid = false;
  try {
    device.createCommandEncoder().beginRenderPass({
      colorAttachments: [{
        view: context.getCurrentTexture().createView(),
        loadOp: "clear",
        storeOp: "store",
      }],
      // @ts-expect-error a depth attachment requires a view.
      depthStencilAttachment: {},
    });
  } catch (caught) {
    invalid = caught instanceof TypeError;
  }
  assert(invalid, "depth attachment");
  const lost = device.lost;
  device.destroy();
  const info = await lost;
  assert(info.reason === "destroyed", "lost reason");
  let lostDevice = false;
  try {
    device.createBuffer({ size: 4, usage: GPUBufferUsage.COPY_DST });
  } catch (caught) {
    lostDevice = errorName(caught) === "InvalidStateError";
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

function clear(device: GPUDevice, texture: GPUTexture, color: GPUColorDict): void {
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

const computeShader = `
@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(1)
fn main() {
  data[0] = data[0] + 1u;
}
`;

function depthShader(z: string, color: string): string {
  return `
@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  const positions = array<vec2f, 3>(
    vec2f(-1.0, -1.0),
    vec2f(3.0, -1.0),
    vec2f(-1.0, 3.0),
  );
  return vec4f(positions[vertexIndex], ${z}, 1.0);
}

@fragment
fn fs() -> @location(0) vec4f {
  return vec4f(${color});
}
`;
}

async function readBytes(
  device: GPUDevice,
  source: GPUBuffer,
  size: number,
): Promise<Uint8Array> {
  const read = device.createBuffer({
    size,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  const encoder = device.createCommandEncoder();
  encoder.copyBufferToBuffer(source, 0, read, 0, size);
  device.queue.submit([encoder.finish()]);
  await read.mapAsync(GPUMapMode.READ);
  const bytes = new Uint8Array(read.getMappedRange()).slice();
  read.unmap();
  return bytes;
}

Yuzuko.test("a compute shader increments a storage buffer", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const module = device.createShaderModule({ code: computeShader });
  const pipeline = await device.createComputePipelineAsync({
    layout: "auto",
    compute: { module, entryPoint: "main" },
  });
  const storage = device.createBuffer({
    size: 4,
    usage:
      GPUBufferUsage.STORAGE |
      GPUBufferUsage.COPY_SRC |
      GPUBufferUsage.COPY_DST,
  });
  device.queue.writeBuffer(storage, 0, new Uint32Array([7]));
  const group = device.createBindGroup({
    layout: pipeline.getBindGroupLayout(0),
    entries: [{ binding: 0, resource: { buffer: storage } }],
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginComputePass();
  pass.setPipeline(pipeline);
  pass.setBindGroup(0, group);
  pass.dispatchWorkgroups(1);
  pass.end();
  device.queue.submit([encoder.finish()]);
  const bytes = await readBytes(device, storage, 4);
  const value = new DataView(bytes.buffer).getUint32(0, true);
  assert(value === 8, `storage ${value}`);
  device.destroy();
});

Yuzuko.test("a nearer fragment wins the depth test", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const color = device.createTexture({
    size: [16, 16],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const depth = device.createTexture({
    size: [16, 16],
    format: "depth24plus",
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  function pipeline(z: string, pixel: string): GPURenderPipeline {
    const module = device.createShaderModule({ code: depthShader(z, pixel) });
    return device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: {
        module,
        entryPoint: "fs",
        targets: [{ format: "rgba8unorm" }],
      },
      depthStencil: {
        format: "depth24plus",
        depthWriteEnabled: true,
        depthCompare: "less",
      },
    });
  }
  const near = pipeline("0.2", "0.0, 1.0, 0.0, 1.0");
  const far = pipeline("0.8", "1.0, 0.0, 0.0, 1.0");
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
    depthStencilAttachment: {
      view: depth.createView(),
      depthClearValue: 1,
      depthLoadOp: "clear",
      depthStoreOp: "store",
    },
  });
  pass.setPipeline(near);
  pass.draw(3);
  pass.setPipeline(far);
  pass.draw(3);
  pass.end();
  device.queue.submit([encoder.finish()]);
  const pixel = channels(await readPixel(device, color, 8, 8), "rgba8unorm");
  assert(pixel.g > 200 && pixel.r < 40, `depth pixel ${pixel.r},${pixel.g}`);
  device.destroy();
});

Yuzuko.test("an occlusion query counts covered samples", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const queries = device.createQuerySet({ type: "occlusion", count: 1 });
  assert(queries.count === 1, "query count");
  assert(queries.type === "occlusion", "query type");
  const color = device.createTexture({
    size: [16, 16],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const module = device.createShaderModule({ code: triangleShader });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
    },
  });
  const resolved = device.createBuffer({
    size: 8,
    usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC,
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
    occlusionQuerySet: queries,
  });
  pass.setPipeline(pipeline);
  pass.beginOcclusionQuery(0);
  pass.draw(3);
  pass.endOcclusionQuery();
  pass.end();
  encoder.resolveQuerySet(queries, 0, 1, resolved, 0);
  device.queue.submit([encoder.finish()]);
  const bytes = await readBytes(device, resolved, 8);
  const view = new DataView(bytes.buffer);
  const samples = view.getUint32(0, true) + view.getUint32(4, true) * 2 ** 32;
  assert(samples > 0, `occlusion ${samples}`);
  device.destroy();
});

Yuzuko.test("timestamp queries follow the device feature", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  if (device.features.has("timestamp-query")) {
    const requested = await adapter.requestDevice({
      requiredFeatures: ["timestamp-query"],
    });
    assert(requested.features.has("timestamp-query"), "requested feature");
    const queries = device.createQuerySet({ type: "timestamp", count: 2 });
    const resolved = device.createBuffer({
      size: 16,
      usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC,
    });
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginComputePass({
      timestampWrites: {
        querySet: queries,
        beginningOfPassWriteIndex: 0,
        endOfPassWriteIndex: 1,
      },
    });
    pass.end();
    encoder.resolveQuerySet(queries, 0, 2, resolved, 0);
    device.queue.submit([encoder.finish()]);
    const bytes = await readBytes(device, resolved, 16);
    assert(bytes.length === 16, "timestamp bytes");
    requested.destroy();
  } else {
    let unsupported = false;
    try {
      device.createQuerySet({ type: "timestamp", count: 1 });
    } catch (caught) {
      unsupported = errorName(caught) === "NotSupportedError";
    }
    assert(unsupported, "timestamp query");
  }
  device.destroy();
});

Yuzuko.test("a render bundle draws the triangle", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const color = device.createTexture({
    size: [16, 16],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const module = device.createShaderModule({ code: triangleShader });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
    },
  });
  const bundles = device.createRenderBundleEncoder({
    colorFormats: ["rgba8unorm"],
  });
  bundles.setPipeline(pipeline);
  bundles.draw(3);
  const bundle = bundles.finish();
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  pass.executeBundles([bundle]);
  pass.end();
  device.queue.submit([encoder.finish()]);
  const pixel = channels(await readPixel(device, color, 8, 8), "rgba8unorm");
  assert(pixel.g > 200 && pixel.r < 40 && pixel.b < 40, "bundle center");
  device.destroy();
});

const coverShader = `
@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  const positions = array<vec2f, 3>(
    vec2f(-1.0, -1.0),
    vec2f(3.0, -1.0),
    vec2f(-1.0, 3.0),
  );
  return vec4f(positions[vertexIndex], 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4f {
  return vec4f(1.0, 0.0, 0.0, 1.0);
}
`;

const storageShader = `
@group(0) @binding(0) var tex: texture_storage_2d<r32uint, read_write>;

@compute @workgroup_size(1)
fn store() {
  textureStore(tex, vec2u(0, 0), vec4u(11u, 0u, 0u, 0u));
}

@compute @workgroup_size(1)
fn bump() {
  let value = textureLoad(tex, vec2u(0, 0));
  textureStore(tex, vec2u(0, 0), value + vec4u(1u, 0u, 0u, 0u));
}
`;

function uintPixel(pixel: ArrayLike<number | undefined>): number {
  return (
    pixel[0]! +
    pixel[1]! * 256 +
    pixel[2]! * 65536 +
    pixel[3]! * 16777216
  );
}

Yuzuko.test(
  "viewport, scissor, multisample resolve, and storage read-write",
  async () => {
    const adapter = await adapterOrSkip();
    if (!adapter) {
      return;
    }
    const device = await adapter.requestDevice();
    const color = device.createTexture({
      size: [8, 8],
      format: "rgba8unorm",
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
    });
    const module = device.createShaderModule({ code: coverShader });
    const pipeline = device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: {
        module,
        entryPoint: "fs",
        targets: [{ format: "rgba8unorm" }],
      },
    });
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: color.createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear",
        storeOp: "store",
      }],
    });
    pass.setViewport(0, 0, 4, 8, 0, 1);
    pass.setScissorRect(2, 0, 6, 8);
    pass.setPipeline(pipeline);
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
    const outsideScissor = channels(
      await readPixel(device, color, 0, 4),
      "rgba8unorm",
    );
    const inside = channels(await readPixel(device, color, 3, 4), "rgba8unorm");
    const outsideViewport = channels(
      await readPixel(device, color, 6, 4),
      "rgba8unorm",
    );
    assert(
      outsideScissor.r < 40 && outsideScissor.g < 40,
      `scissor ${outsideScissor.r}`,
    );
    assert(inside.r > 200 && inside.g < 40, `viewport ${inside.r}`);
    assert(
      outsideViewport.r < 40 && outsideViewport.g < 40,
      `viewport edge ${outsideViewport.r}`,
    );

    const multisample = device.createTexture({
      size: [8, 8],
      format: "rgba8unorm",
      sampleCount: 4,
      usage: GPUTextureUsage.RENDER_ATTACHMENT,
    });
    const resolved = device.createTexture({
      size: [8, 8],
      format: "rgba8unorm",
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
    });
    const triangle = device.createShaderModule({ code: triangleShader });
    const resolvedPipeline = device.createRenderPipeline({
      layout: "auto",
      multisample: { count: 4 },
      vertex: { module: triangle, entryPoint: "vs" },
      fragment: {
        module: triangle,
        entryPoint: "fs",
        targets: [{ format: "rgba8unorm" }],
      },
    });
    const resolveEncoder = device.createCommandEncoder();
    const resolvePass = resolveEncoder.beginRenderPass({
      colorAttachments: [{
        view: multisample.createView(),
        resolveTarget: resolved.createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear",
        storeOp: "discard",
      }],
    });
    resolvePass.setPipeline(resolvedPipeline);
    resolvePass.draw(3);
    resolvePass.end();
    device.queue.submit([resolveEncoder.finish()]);
    const center = channels(
      await readPixel(device, resolved, 4, 4),
      "rgba8unorm",
    );
    const corner = channels(
      await readPixel(device, resolved, 0, 0),
      "rgba8unorm",
    );
    assert(center.g > 200 && center.r < 40, `resolve center ${center.g}`);
    assert(corner.g < 40 && corner.r < 40, `resolve corner ${corner.g}`);

    const storageModule = device.createShaderModule({ code: storageShader });
    const storageLayout = device.createBindGroupLayout({
      entries: [{
        binding: 0,
        visibility: GPUShaderStage.COMPUTE,
        storageTexture: { access: "read-write", format: "r32uint" },
      }],
    });
    const pipelineLayout = device.createPipelineLayout({
      bindGroupLayouts: [storageLayout],
    });
    const storePipeline = device.createComputePipeline({
      layout: pipelineLayout,
      compute: { module: storageModule, entryPoint: "store" },
    });
    const bumpPipeline = device.createComputePipeline({
      layout: pipelineLayout,
      compute: { module: storageModule, entryPoint: "bump" },
    });
    const storage = device.createTexture({
      size: [1, 1],
      format: "r32uint",
      usage: GPUTextureUsage.STORAGE_BINDING | GPUTextureUsage.COPY_SRC,
    });
    const group = device.createBindGroup({
      layout: storageLayout,
      entries: [{ binding: 0, resource: storage.createView() }],
    });
    const storageEncoder = device.createCommandEncoder();
    const storePass = storageEncoder.beginComputePass();
    storePass.setPipeline(storePipeline);
    storePass.setBindGroup(0, group);
    storePass.dispatchWorkgroups(1);
    storePass.end();
    const bumpPass = storageEncoder.beginComputePass();
    bumpPass.setPipeline(bumpPipeline);
    bumpPass.setBindGroup(0, group);
    bumpPass.dispatchWorkgroups(1);
    bumpPass.end();
    device.queue.submit([storageEncoder.finish()]);
    const stored = uintPixel(await readPixel(device, storage, 0, 0));
    assert(stored === 12, `storage ${stored}`);

    device.pushErrorScope("validation");
    device.createBindGroupLayout({
      entries: [{
        binding: 0,
        visibility: GPUShaderStage.COMPUTE,
        storageTexture: { access: "read-write", format: "rgba8unorm" },
      }],
    });
    const rejected = await device.popErrorScope();
    assert(rejected instanceof GPUValidationError, "rgba8unorm read-write");
    device.pushErrorScope("validation");
    device.createBindGroupLayout({
      entries: [{
        binding: 0,
        visibility: GPUShaderStage.COMPUTE,
        storageTexture: { access: "atomic", format: "r32uint" },
      }],
    });
    const atomic = await device.popErrorScope();
    assert(atomic instanceof GPUValidationError, "atomic storage");

    let unsupported = false;
    try {
      device.createRenderPipeline({
        layout: "auto",
        vertex: { module, entryPoint: "vs" },
        fragment: {
          module,
          entryPoint: "fs",
          targets: [{
            format: "rgba8unorm",
            blend: {
              color: { srcFactor: "src1" },
              alpha: {},
            },
          }],
        },
      });
    } catch (error) {
      unsupported = errorName(error) === "NotSupportedError";
    }
    assert(
      !device.features.has("dual-source-blending") && unsupported,
      "dual source blending",
    );
    let invalidAccess = false;
    try {
      device.createBindGroupLayout({
        entries: [{
          binding: 0,
          visibility: GPUShaderStage.COMPUTE,
          storageTexture: {
            // @ts-expect-error nope is not a storage-texture access.
            access: "nope",
            format: "r32uint",
          },
        }],
      });
    } catch (error) {
      invalidAccess = error instanceof TypeError;
    }
    assert(invalidAccess, "storage access");
    device.destroy();
  },
);

Yuzuko.test("GPU object labels round-trip", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice({ label: "device" });
  assert(device.label === "device", device.label);
  device.label = "device-2";
  assert(device.label === "device-2", "device set");
  assert(device.queue.label === "", "queue starts empty");
  device.queue.label = "queue";
  assert(device.queue.label === "queue", "queue set");
  const buffer = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST,
    label: "buf",
  });
  assert(buffer.label === "buf", buffer.label);
  buffer.label = "buf2";
  assert(buffer.label === "buf2", "buffer set");
  const texture = device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.TEXTURE_BINDING,
    label: "tex",
  });
  assert(texture.label === "tex", texture.label);
  const view = texture.createView({ label: "view" });
  assert(view.label === "view", view.label);
  const encoder = device.createCommandEncoder({ label: "enc" });
  assert(encoder.label === "enc", encoder.label);
  const pass = encoder.beginComputePass({ label: "pass" });
  assert(pass.label === "pass", pass.label);
  pass.end();
  const command = encoder.finish({ label: "cmd" });
  assert(command.label === "cmd", command.label);
  const canvas = new OffscreenCanvas(4, 4);
  const context = canvas.getContext("webgpu")!;
  assert(context.label === "", "canvas label");
  let threw = false;
  try {
    // @ts-expect-error label is read-only.
    context.label = "nope";
  } catch (error) {
    threw = error instanceof TypeError;
  }
  assert(threw, "canvas label assignment");
  assert(context.label === "", "canvas label stays empty");
  device.destroy();
});

const alphaBlend: GPUBlendState = {
  color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" },
  alpha: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" },
};

function colorTexture(device: GPUDevice): GPUTexture {
  return device.createTexture({
    size: [16, 16],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
}

async function overdraw(device: GPUDevice, blend: GPUBlendState | null) {
  const color = colorTexture(device);
  const red = device.createShaderModule({
    code: depthShader("0.0", "1.0, 0.0, 0.0, 1.0"),
  });
  const green = device.createShaderModule({
    code: depthShader("0.0", "0.0, 1.0, 0.0, 0.5"),
  });
  function pipeline(module: GPUShaderModule): GPURenderPipeline {
    const target: GPUColorTargetState = { format: "rgba8unorm" };
    if (blend) {
      target.blend = blend;
    }
    return device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: { module, entryPoint: "fs", targets: [target] },
    });
  }
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  pass.setPipeline(pipeline(red));
  pass.draw(3);
  pass.setPipeline(pipeline(green));
  pass.draw(3);
  pass.end();
  device.queue.submit([encoder.finish()]);
  return channels(await readPixel(device, color, 8, 8), "rgba8unorm");
}

Yuzuko.test("blend changes a read-back pixel", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const plain = await overdraw(device, null);
  const mixed = await overdraw(device, alphaBlend);
  assert(plain.g > 200 && plain.r < 40, `replace ${plain.r},${plain.g}`);
  assert(mixed.r > 80 && mixed.g > 80, `blend ${mixed.r},${mixed.g}`);
  assert(Math.abs(mixed.r - plain.r) > 40, "blend differs");
  device.destroy();
});

async function coveredPixel(device: GPUDevice, alpha: string) {
  const multisample = device.createTexture({
    size: [8, 8],
    format: "rgba8unorm",
    sampleCount: 4,
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  const resolved = device.createTexture({
    size: [8, 8],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const module = device.createShaderModule({
    code: depthShader("0.0", `1.0, 0.0, 0.0, ${alpha}`),
  });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    multisample: { count: 4, alphaToCoverageEnabled: true },
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
    },
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: multisample.createView(),
      resolveTarget: resolved.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "discard",
    }],
  });
  pass.setPipeline(pipeline);
  pass.draw(3);
  pass.end();
  device.queue.submit([encoder.finish()]);
  return channels(await readPixel(device, resolved, 4, 4), "rgba8unorm");
}

Yuzuko.test("alpha to coverage resolves covered samples", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const covered = await coveredPixel(device, "1.0");
  const missed = await coveredPixel(device, "0.0");
  assert(covered.r > 200, `coverage ${covered.r}`);
  assert(missed.r < 40, `no coverage ${missed.r}`);
  device.destroy();
});

Yuzuko.test("unclipped depth keeps a fragment outside the clip volume", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const module = device.createShaderModule({
    code: depthShader("2.0", "1.0, 0.0, 0.0, 1.0"),
  });
  device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
    },
    primitive: { unclippedDepth: false },
  });
  if (!device.features.has("depth-clip-control")) {
    let threw = false;
    try {
      device.createRenderPipeline({
        layout: "auto",
        vertex: { module, entryPoint: "vs" },
        fragment: {
          module,
          entryPoint: "fs",
          targets: [{ format: "rgba8unorm" }],
        },
        primitive: { unclippedDepth: true },
      });
    } catch (error) {
      threw = errorName(error) === "NotSupportedError";
    }
    assert(threw, "unclippedDepth");
    device.destroy();
    return;
  }
  function pipeline(unclipped: boolean): GPURenderPipeline {
    return device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: {
        module,
        entryPoint: "fs",
        targets: [{ format: "rgba8unorm" }],
      },
      primitive: { unclippedDepth: unclipped },
    });
  }
  async function pixel(unclipped: boolean) {
    const color = colorTexture(device);
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: color.createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear",
        storeOp: "store",
      }],
    });
    pass.setPipeline(pipeline(unclipped));
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
    return channels(await readPixel(device, color, 8, 8), "rgba8unorm");
  }
  const clipped = await pixel(false);
  const kept = await pixel(true);
  assert(clipped.r < 40, `clipped ${clipped.r}`);
  assert(kept.r > 200, `unclipped ${kept.r}`);
  device.destroy();
});

Yuzuko.test("depth32float-stencil8 rejects a farther fragment", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  if (!device.features.has("depth32float-stencil8")) {
    let threw = false;
    try {
      device.createTexture({
        size: [4, 4],
        format: "depth32float-stencil8",
        usage: GPUTextureUsage.RENDER_ATTACHMENT,
      });
    } catch (error) {
      threw = errorName(error) === "NotSupportedError";
    }
    assert(threw, "depth32float-stencil8");
    device.destroy();
    return;
  }
  const color = colorTexture(device);
  const depth = device.createTexture({
    size: [16, 16],
    format: "depth32float-stencil8",
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  function pipeline(z: string, pixel: string): GPURenderPipeline {
    const module = device.createShaderModule({ code: depthShader(z, pixel) });
    return device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: {
        module,
        entryPoint: "fs",
        targets: [{ format: "rgba8unorm" }],
      },
      depthStencil: {
        format: "depth32float-stencil8",
        depthWriteEnabled: true,
        depthCompare: "less",
      },
    });
  }
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
    depthStencilAttachment: {
      view: depth.createView(),
      depthClearValue: 1,
      depthLoadOp: "clear",
      depthStoreOp: "store",
      stencilLoadOp: "clear",
      stencilStoreOp: "store",
    },
  });
  pass.setPipeline(pipeline("0.2", "0.0, 1.0, 0.0, 1.0"));
  pass.draw(3);
  pass.setPipeline(pipeline("0.8", "1.0, 0.0, 0.0, 1.0"));
  pass.draw(3);
  pass.end();
  device.queue.submit([encoder.finish()]);
  const pixel = channels(await readPixel(device, color, 8, 8), "rgba8unorm");
  assert(pixel.g > 200 && pixel.r < 40, `depth32 ${pixel.r},${pixel.g}`);
  device.destroy();
});

Yuzuko.test("destroying a query set makes resolve a validation error", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const queries = device.createQuerySet({ type: "occlusion", count: 1 });
  queries.destroy();
  queries.destroy();
  const resolved = device.createBuffer({
    size: 8,
    usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC,
  });
  device.pushErrorScope("validation");
  const encoder = device.createCommandEncoder();
  encoder.resolveQuerySet(queries, 0, 1, resolved, 0);
  device.queue.submit([encoder.finish()]);
  const error = await device.popErrorScope();
  assert(error instanceof GPUValidationError, "destroyed query");
  device.destroy();
});

const externalShader = `
@group(0) @binding(0) var tex: texture_external;
@group(0) @binding(1) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  const positions = array<vec2f, 3>(
    vec2f(-1.0, -1.0),
    vec2f(3.0, -1.0),
    vec2f(-1.0, 3.0),
  );
  return vec4f(positions[vertexIndex], 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4f {
  return textureSampleBaseClampToEdge(tex, samp, vec2f(0.5, 0.5));
}
`;

Yuzuko.test("an imported video frame samples as an external texture", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const frame = new VideoFrame(new Uint8Array([255, 0, 0, 255]), {
    format: "RGBA",
    codedWidth: 1,
    codedHeight: 1,
    timestamp: 0,
  });
  let external = null;
  try {
    external = device.importExternalTexture({ source: frame });
  } catch (error) {
    assert(
      errorName(error) === "NotSupportedError",
      `external import ${errorName(error)}`,
    );
    let layoutThrew = false;
    try {
      device.createBindGroupLayout({
        entries: [{
          binding: 0,
          visibility: GPUShaderStage.FRAGMENT,
          externalTexture: {},
        }],
      });
    } catch (layoutError) {
      layoutThrew = errorName(layoutError) === "NotSupportedError";
    }
    assert(layoutThrew, "external layout");
    frame.close();
    device.destroy();
    return;
  }
  const module = device.createShaderModule({ code: externalShader });
  const layout = device.createBindGroupLayout({
    entries: [
      {
        binding: 0,
        visibility: GPUShaderStage.FRAGMENT,
        externalTexture: {},
      },
      {
        binding: 1,
        visibility: GPUShaderStage.FRAGMENT,
        sampler: { type: "filtering" },
      },
    ],
  });
  const pipeline = device.createRenderPipeline({
    layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
    },
  });
  const sampler = device.createSampler({
    magFilter: "linear",
    minFilter: "linear",
  });
  const group = device.createBindGroup({
    layout,
    entries: [
      { binding: 0, resource: external },
      { binding: 1, resource: sampler },
    ],
  });
  const color = device.createTexture({
    size: [4, 4],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  pass.setPipeline(pipeline);
  pass.setBindGroup(0, group);
  pass.draw(3);
  pass.end();
  device.queue.submit([encoder.finish()]);
  const pixel = channels(await readPixel(device, color, 1, 1), "rgba8unorm");
  assert(pixel.r > 200 && pixel.g < 40 && pixel.b < 40, `frame ${pixel.r}`);
  frame.close();
  let closed = false;
  try {
    device.importExternalTexture({ source: frame });
  } catch (error) {
    closed = errorName(error) === "InvalidStateError";
  }
  assert(closed, "closed frame");
  let typeError = false;
  try {
    device.importExternalTexture({
      // @ts-expect-error source must be a VideoFrame.
      source: {},
    });
  } catch (error) {
    typeError = error instanceof TypeError;
  }
  assert(typeError, "external source");
  device.destroy();
});

Yuzuko.test("debug markers on the command encoder and on passes", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const encoder = device.createCommandEncoder();
  encoder.pushDebugGroup("group");
  encoder.insertDebugMarker("mark");
  encoder.insertDebugMarker("");
  encoder.popDebugGroup();
  device.pushErrorScope("validation");
  encoder.popDebugGroup();
  encoder.finish();
  const empty = await device.popErrorScope();
  assert(empty instanceof GPUValidationError, "empty command pop");

  const color = device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  const drawing = device.createCommandEncoder();
  const pass = drawing.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  pass.pushDebugGroup("draw");
  pass.insertDebugMarker("here");
  pass.popDebugGroup();
  pass.end();
  const compute = drawing.beginComputePass();
  compute.pushDebugGroup("dispatch");
  compute.insertDebugMarker("");
  compute.popDebugGroup();
  compute.end();
  device.queue.submit([drawing.finish()]);

  const late = device.createCommandEncoder();
  const latePass = late.beginRenderPass({
    colorAttachments: [{
      view: color.createView(),
      loadOp: "load",
      storeOp: "store",
    }],
  });
  device.pushErrorScope("validation");
  latePass.popDebugGroup();
  latePass.end();
  late.finish();
  const passError = await device.popErrorScope();
  assert(passError instanceof GPUValidationError, "empty pass pop");

  const computeEncoder = device.createCommandEncoder();
  const lateCompute = computeEncoder.beginComputePass();
  device.pushErrorScope("validation");
  lateCompute.popDebugGroup();
  lateCompute.end();
  computeEncoder.finish();
  const computeError = await device.popErrorScope();
  assert(computeError instanceof GPUValidationError, "empty compute pop");

  const bundle = device.createRenderBundleEncoder({
    colorFormats: ["rgba8unorm"],
  });
  assert(
    throwName(() => bundle.pushDebugGroup("group")) === "NotSupportedError",
    "bundle markers",
  );
  assert(
    throwName(() => bundle.setViewport(0, 0, 1, 1, 0, 1)) === "NotSupportedError",
    "bundle viewport",
  );
  bundle.finish();
  device.destroy();
});

Yuzuko.test("wgslLanguageFeatures lists the implemented extensions", () => {
  const names = [...navigator.gpu.wgslLanguageFeatures];
  assert(
    names.includes("readonly_and_readwrite_storage_textures"),
    "storage textures",
  );
  assert(names.includes("packed_4x8_integer_dot_product"), "dot product");
  assert(names.includes("pointer_composite_access"), "pointer composite");
  assert(
    !names.includes("unrestricted_pointer_parameters"),
    "unimplemented extension",
  );
});

Yuzuko.test("GPUUncapturedErrorEvent carries a GPU error", () => {
  const error = new GPUValidationError("nope");
  const event = new GPUUncapturedErrorEvent("uncapturederror", { error });
  assert(event instanceof GPUUncapturedErrorEvent, "event");
  assert(event.error === error, "error");
  assert(event.isTrusted === false, "script event");
  let missing = false;
  try {
    new GPUUncapturedErrorEvent(
      "uncapturederror",
      // @ts-expect-error error is required.
      {},
    );
  } catch (caught) {
    missing = caught instanceof TypeError;
  }
  assert(missing, "required error");
});

Yuzuko.test("mapAsync writes back and detaches the range", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const buffer = device.createBuffer({
    size: 16,
    usage: GPUBufferUsage.MAP_WRITE | GPUBufferUsage.COPY_SRC,
  });
  await buffer.mapAsync(GPUMapMode.WRITE);
  const mapped = buffer.getMappedRange();
  new Uint8Array(mapped).set([
    9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2, 3, 4, 5, 6,
  ]);
  buffer.unmap();
  assert(mapped.byteLength === 0, "detached");
  const bytes = await readBytes(device, buffer, 16);
  assert(bytes[0] === 9 && bytes[15] === 6, `written ${bytes}`);

  await buffer.mapAsync(GPUMapMode.WRITE);
  buffer.getMappedRange(0, 8);
  let overlap = false;
  try {
    buffer.getMappedRange(4, 8);
  } catch (caught) {
    overlap = errorName(caught) === "OperationError";
  }
  assert(overlap, "overlap");
  let align = false;
  try {
    buffer.getMappedRange(4, 4);
  } catch (caught) {
    align = errorName(caught) === "OperationError";
  }
  assert(align, "offset alignment");
  buffer.unmap();

  let badOffset = false;
  try {
    await buffer.mapAsync(GPUMapMode.WRITE, 4, 4);
  } catch (caught) {
    badOffset = errorName(caught) === "OperationError";
  }
  assert(badOffset, "map offset");
  device.destroy();
});

Yuzuko.test("writeTimestamp follows the timestamp feature", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const encoder = device.createCommandEncoder();
  if (!device.features.has("timestamp-query")) {
    let unsupported = false;
    try {
      encoder.writeTimestamp(
        device.createQuerySet({ type: "occlusion", count: 1 }),
        0,
      );
    } catch (caught) {
      unsupported = errorName(caught) === "NotSupportedError";
    }
    assert(unsupported, "timestamp write");
    device.destroy();
    return;
  }
  const queries = device.createQuerySet({ type: "timestamp", count: 4 });
  let inside = true;
  try {
    encoder.writeTimestamp(queries, 0);
  } catch (caught) {
    assert(errorName(caught) === "NotSupportedError", "inside encoder");
    inside = false;
  }
  if (inside) {
    const compute = encoder.beginComputePass();
    compute.writeTimestamp(queries, 1);
    compute.end();
    const color = device.createTexture({
      size: [1, 1],
      format: "rgba8unorm",
      usage: GPUTextureUsage.RENDER_ATTACHMENT,
    });
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: color.createView(),
        loadOp: "clear",
        storeOp: "store",
      }],
    });
    pass.writeTimestamp(queries, 2);
    pass.end();
    const occlusion = device.createQuerySet({ type: "occlusion", count: 1 });
    let typed = false;
    try {
      encoder.writeTimestamp(occlusion, 0);
    } catch (caught) {
      typed = caught instanceof TypeError;
    }
    assert(typed, "occlusion query set");
    device.queue.submit([encoder.finish()]);
    queries.destroy();
    const again = device.createCommandEncoder();
    device.pushErrorScope("validation");
    again.writeTimestamp(queries, 0);
    device.queue.submit([again.finish()]);
    const error = await device.popErrorScope();
    assert(error instanceof GPUValidationError, "destroyed timestamp");
  }
  device.destroy();
});

Yuzuko.test("an uncaptured validation error is an event", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  assert(device instanceof EventTarget, "event target");
  const source = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.COPY_SRC,
  });
  const destination = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.COPY_DST,
  });
  let uncaptured = 0;
  device.onuncapturederror = (event) => {
    if (event instanceof GPUUncapturedErrorEvent
      && event.error instanceof GPUValidationError
      && event.isTrusted === true) {
      uncaptured += 1;
    }
  };
  device.pushErrorScope("validation");
  const scoped = device.createCommandEncoder();
  scoped.copyBufferToBuffer(source, 0, destination, 0, 16);
  device.queue.submit([scoped.finish()]);
  const captured = await device.popErrorScope();
  assert(captured instanceof GPUValidationError, "scoped validation");
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  assert(uncaptured === 0, "scope captured the error");

  const seen = new Promise<Event>((resolve) => {
    device.addEventListener("uncapturederror", resolve, { once: true });
  });
  const encoder = device.createCommandEncoder();
  encoder.copyBufferToBuffer(source, 0, destination, 0, 16);
  device.queue.submit([encoder.finish()]);
  const event = await Promise.race([
    seen,
    new Promise<Event | null>((resolve) => setTimeout(() => resolve(null), 1000)),
  ]);
  assert(event instanceof GPUUncapturedErrorEvent, "event type");
  if (!(event instanceof GPUUncapturedErrorEvent)) {
    throw new Error("event type");
  }
  assert(event.error instanceof GPUValidationError, "validation");
  assert(event.isTrusted === true, "trusted");
  assert(uncaptured >= 1, "handler");
  device.destroy();
});

function throwName(fn: () => void): string {
  try {
    fn();
    return "";
  } catch (error) {
    return errorName(error);
  }
}

async function rejectionName(promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
    return "";
  } catch (error) {
    return errorName(error);
  }
}

Yuzuko.test("published WebGPU members match the created objects", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  for (const value of [adapter.info, device.adapterInfo]) {
    assert(typeof value.vendor === "string", "vendor");
    assert(typeof value.device === "string", "device");
    assert(value.architecture === "", "architecture");
    assert(typeof value.description === "string", "description");
    assert(typeof value.subgroupMinSize === "number", "subgroup min");
    assert(typeof value.subgroupMaxSize === "number", "subgroup max");
    assert(value.isFallbackAdapter === false, "fallback");
  }
  const limits = adapter.limits;
  assert(
    limits.maxStorageBuffersInVertexStage === limits.maxStorageBuffersPerShaderStage,
    "vertex buffers",
  );
  assert(
    limits.maxStorageBuffersInFragmentStage === limits.maxStorageBuffersPerShaderStage,
    "fragment buffers",
  );
  assert(
    limits.maxStorageTexturesInVertexStage === limits.maxStorageTexturesPerShaderStage,
    "vertex textures",
  );
  assert(
    limits.maxStorageTexturesInFragmentStage === limits.maxStorageTexturesPerShaderStage,
    "fragment textures",
  );
  assert(
    device.limits.maxStorageBuffersInVertexStage
      === device.limits.maxStorageBuffersPerShaderStage,
    "device vertex buffers",
  );
  assert(
    device.limits.maxStorageTexturesInFragmentStage
      === device.limits.maxStorageTexturesPerShaderStage,
    "device fragment textures",
  );

  const usage = GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST;
  const buffer = device.createBuffer({ size: 16, usage });
  assert(buffer.usage === usage, "buffer usage");
  assert(buffer.mapState === "unmapped", "unmapped");
  const mapping = buffer.mapAsync(GPUMapMode.READ);
  assert(buffer.mapState === "mapped", "mapped on the calling stack");
  await mapping;
  buffer.unmap();
  assert(buffer.mapState === "unmapped", "unmapped after unmap");

  const textureUsage = GPUTextureUsage.COPY_DST | GPUTextureUsage.TEXTURE_BINDING;
  const texture = device.createTexture({
    size: [4, 4],
    format: "rgba8unorm",
    usage: textureUsage,
  });
  assert(texture.dimension === "2d", "dimension");
  assert(texture.depthOrArrayLayers === 1, "layers");
  assert(texture.mipLevelCount === 1, "mips");
  assert(texture.sampleCount === 1, "samples");
  assert(texture.usage === textureUsage, "texture usage");
  texture.createView();
  texture.createView({ swizzle: "rgba" });
  assert(
    throwName(() => texture.createView({ swizzle: "bgra" })) === "NotSupportedError",
    "swizzle",
  );
  assert(
    throwName(() => texture.createView({ usage: 0 })) === "NotSupportedError",
    "view usage",
  );
  assert(throwName(() => device.createSampler({ compare: "less" })) === "NotSupportedError", "compare");
  device.createSampler();

  const attachment = texture.createView();
  const passEncoder = device.createCommandEncoder();
  assert(
    throwName(() => {
      passEncoder.beginRenderPass({
        colorAttachments: [{
          view: attachment,
          loadOp: "clear",
          storeOp: "store",
          depthSlice: 0,
        }],
      });
    }) === "NotSupportedError",
    "depth slice",
  );

  const encoder = device.createCommandEncoder();
  assert(throwName(() => encoder.clearBuffer(buffer)) === "NotSupportedError", "clearBuffer");
  assert(
    throwName(() => encoder.copyBufferToTexture(
      // @ts-expect-error the copy source requires a buffer.
      {},
      {},
      [1, 1],
    )) === "NotSupportedError",
    "copyBufferToTexture",
  );
  assert(
    throwName(() => encoder.copyTextureToTexture(
      // @ts-expect-error the copy source requires a texture.
      {},
      {},
      [1, 1],
    )) === "NotSupportedError",
    "copyTextureToTexture",
  );
  assert(
    throwName(() => {
      device.queue.writeTexture(
        // @ts-expect-error the destination requires a texture.
        {},
        new Uint8Array(4),
        {},
        [1, 1],
      );
    }) === "NotSupportedError",
    "writeTexture",
  );

  const drawing = device.createCommandEncoder();
  const pass = drawing.beginRenderPass({
    colorAttachments: [{
      view: texture.createView(),
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  assert(throwName(() => pass.setBlendConstant([0, 0, 0, 0])) === "NotSupportedError", "blend");
  assert(throwName(() => pass.setStencilReference(0)) === "NotSupportedError", "stencil");

  const shader = device.createShaderModule({ code: triangleShader });
  assert(
    await rejectionName(shader.getCompilationInfo()) === "NotSupportedError",
    "compilation info",
  );
  assert(
    await rejectionName(device.createRenderPipelineAsync(
      // @ts-expect-error layout and vertex are required.
      {},
    )) === "NotSupportedError",
    "render pipeline async",
  );
  assert(
    await rejectionName(device.queue.onSubmittedWorkDone()) === "NotSupportedError",
    "submitted work",
  );

  const canvas = new OffscreenCanvas(4, 4);
  const context = canvas.getContext("webgpu")!;
  assert(context !== null, "webgpu context");
  assert(context.getConfiguration() === null, "unconfigured");
  const format = navigator.gpu.getPreferredCanvasFormat();
  context.configure({ device, format });
  const configuration = context.getConfiguration();
  assert(configuration !== null, "configured");
  if (configuration === null) {
    throw new Error("configured");
  }
  assert(configuration.device === device, "config device");
  assert(configuration.format === format, "config format");
  assert(configuration.alphaMode === "opaque", "alpha");
  assert(configuration.colorSpace === "srgb", "color space");
  assert(configuration.toneMapping!.mode === "standard", "tone mapping");
  assert(Array.isArray(configuration.viewFormats), "view formats");
  assert(configuration.usage === GPUTextureUsage.RENDER_ATTACHMENT, "config usage");
  const current = context.getCurrentTexture();
  const canvasUsage = GPUTextureUsage.RENDER_ATTACHMENT
    | GPUTextureUsage.TEXTURE_BINDING
    | GPUTextureUsage.COPY_SRC;
  assert((current.usage & canvasUsage) === canvasUsage, "canvas usage");
  assert(current.dimension === "2d", "canvas dimension");
  assert(current.mipLevelCount === 1, "canvas mips");
  assert(current.depthOrArrayLayers === 1, "canvas layers");
  assert(current.sampleCount === 1, "canvas samples");
  context.configure({
    device,
    format,
    toneMapping: { mode: "standard" },
  });
  assert(
    context.getConfiguration()!.toneMapping!.mode === "standard",
    "standard tone mapping",
  );
  assert(
    throwName(() => {
      context.configure({
        device,
        format,
        toneMapping: { mode: "extended" },
      });
    }) === "NotSupportedError",
    "extended tone mapping",
  );

  buffer.destroy();
  assert(buffer.usage === usage, "usage after destroy");
  assert(buffer.mapState === "unmapped", "unmapped after destroy");
  texture.destroy();
  assert(texture.usage === textureUsage, "texture usage after destroy");
  assert(texture.sampleCount === 1, "samples after destroy");
  device.destroy();
});

Yuzuko.test("ImageData copies into a texture", async () => {
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  const device = await adapter.requestDevice();
  const data = new ImageData(new Uint8ClampedArray([0, 255, 0, 255]), 1, 1);
  const texture = device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.COPY_DST | GPUTextureUsage.COPY_SRC,
  });
  device.queue.copyExternalImageToTexture(
    { source: data },
    { texture },
    [1, 1],
  );
  const pixel = channels(await readPixel(device, texture, 0, 0), "rgba8unorm");
  assert(pixel.g === 255 && pixel.r === 0 && pixel.b === 0 && pixel.a === 255, "green");

  const bytes = new Uint8ClampedArray([0, 255, 0, 255]);
  const detached = new ImageData(bytes, 1, 1);
  structuredClone(bytes.buffer, { transfer: [bytes.buffer] });
  assert(
    throwName(() => {
      device.queue.copyExternalImageToTexture(
        { source: detached },
        { texture },
        [1, 1],
      );
    }) === "InvalidStateError",
    "detached ImageData",
  );
  device.destroy();
});

Yuzuko.test("WebGPU adapter, queue, and pipeline options", async () => {
  assert(GPUTextureUsage.TRANSIENT_ATTACHMENT === 0x20, "transient bit");
  const adapter = await adapterOrSkip();
  if (!adapter) {
    return;
  }
  assert(await navigator.gpu.requestAdapter({ featureLevel: "core" }) !== null, "core");
  assert(await navigator.gpu.requestAdapter({ featureLevel: "compatibility" }) === null, "compatibility");
  assert(await navigator.gpu.requestAdapter({ xrCompatible: true }) === null, "xr");
  assert(await navigator.gpu.requestAdapter({ xrCompatible: false }) !== null, "xr false");
  assert(
    await rejectionName(navigator.gpu.requestAdapter({ featureLevel: "extended" })) === "TypeError",
    "feature level",
  );
  const level = {
    toString() {
      throw new RangeError("level");
    },
  };
  assert(
    await rejectionName(navigator.gpu.requestAdapter({
      // @ts-expect-error featureLevel is converted with ToString.
      featureLevel: level,
    })) === "RangeError",
    "feature level throw",
  );

  const plain = await adapter.requestDevice();
  assert(plain.queue.label === "", "omitted queue label");
  plain.destroy();
  assert(
    await rejectionName(adapter.requestDevice({
      // @ts-expect-error defaultQueue is not null.
      defaultQueue: null,
    })) === "TypeError",
    "null queue",
  );
  const device = await adapter.requestDevice({ defaultQueue: { label: "frames" } });
  assert(device.queue.label === "frames", "queue label");

  device.pushErrorScope("validation");
  device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TRANSIENT_ATTACHMENT,
  });
  assert(await device.popErrorScope() === null, "transient texture");
  device.pushErrorScope("validation");
  device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.COPY_DST | GPUTextureUsage.TRANSIENT_ATTACHMENT,
  });
  assert((await device.popErrorScope()) instanceof GPUValidationError, "bad transient");

  const layout = device.createPipelineLayout({ bindGroupLayouts: [] });
  device.createShaderModule({
    code: triangleShader,
    compilationHints: [
      { entryPoint: "vs", layout: "auto" },
      { entryPoint: "fs", layout },
    ],
  });
  assert(
    throwName(() => {
      device.createShaderModule({
        code: triangleShader,
        compilationHints: [{
          entryPoint: "vs",
          // @ts-expect-error a plain object is not a GPUPipelineLayout.
          layout: {},
        }],
      });
    }) === "TypeError",
    "hint layout",
  );
  assert(
    throwName(() => {
      device.createShaderModule({
        code: triangleShader,
        compilationHints: [
          // @ts-expect-error a hint requires an entryPoint.
          {},
        ],
      });
    }) === "TypeError",
    "hint entry",
  );
  assert(
    throwName(() => {
      device.createShaderModule({
        code: triangleShader,
        // @ts-expect-error compilationHints is not null.
        compilationHints: null,
      });
    }) === "TypeError",
    "hint null",
  );
  device.createPipelineLayout({ bindGroupLayouts: [], immediateSize: 0 });
  assert(
    throwName(() => device.createPipelineLayout({
      bindGroupLayouts: [],
      immediateSize: 4,
    })) === "NotSupportedError",
    "immediateSize",
  );

  const vertexModule = device.createShaderModule({
    code: `
      override bias: f32 = 1.0;
      @vertex
      fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
        const positions = array<vec2f, 3>(
          vec2f(-1.0, -1.0),
          vec2f(3.0, -1.0),
          vec2f(-1.0, 3.0),
        );
        return vec4f(positions[vertexIndex] + vec2f(bias, 0.0), 0.0, 1.0);
      }
    `,
  });
  const fragmentModule = device.createShaderModule({
    code: `
      override amount: f32 = 0.0;
      @fragment
      fn fs() -> @location(0) vec4f {
        return vec4f(0.0, amount, 0.0, 1.0);
      }
    `,
  });
  const painted = device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module: vertexModule, entryPoint: "vs", constants: { bias: 0 } },
    fragment: {
      module: fragmentModule,
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
      constants: { amount: 1 },
    },
  });
  const drawing = device.createCommandEncoder();
  const drawPass = drawing.beginRenderPass({
    colorAttachments: [{
      view: painted.createView(),
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  drawPass.setPipeline(pipeline);
  drawPass.draw(3);
  drawPass.end();
  device.queue.submit([drawing.finish()]);
  const paintedPixel = channels(await readPixel(device, painted, 0, 0), "rgba8unorm");
  assert(paintedPixel.g === 255 && paintedPixel.r === 0, "override green");
  assert(
    throwName(() => {
      device.createRenderPipeline({
        layout: "auto",
        vertex: {
          module: vertexModule,
          entryPoint: "vs",
          constants: {
            // @ts-expect-error a pipeline constant is a number.
            bias: "nope",
          },
        },
        fragment: {
          module: fragmentModule,
          entryPoint: "fs",
          targets: [{ format: "rgba8unorm" }],
        },
      });
    }) === "TypeError",
    "constant",
  );

  const computeModule = device.createShaderModule({
    code: `
      override factor: f32 = 1.0;
      @group(0) @binding(0) var<storage, read_write> values: array<f32>;
      @compute @workgroup_size(1)
      fn main() {
        values[0] = factor;
      }
    `,
  });
  const compute = device.createComputePipeline({
    layout: "auto",
    compute: { module: computeModule, entryPoint: "main", constants: { factor: 0.5 } },
  });
  const storage = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
  });
  const readback = device.createBuffer({
    size: 4,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  const bind = device.createBindGroup({
    layout: compute.getBindGroupLayout(0),
    entries: [{ binding: 0, resource: { buffer: storage } }],
  });
  const computeEncoder = device.createCommandEncoder();
  const computePass = computeEncoder.beginComputePass();
  computePass.setPipeline(compute);
  computePass.setBindGroup(0, bind);
  computePass.dispatchWorkgroups(1);
  computePass.end();
  computeEncoder.copyBufferToBuffer(storage, 0, readback, 0, 4);
  device.queue.submit([computeEncoder.finish()]);
  await readback.mapAsync(GPUMapMode.READ);
  const factor = new Float32Array(readback.getMappedRange())[0];
  readback.unmap();
  assert(factor === 0.5, "compute factor");

  const attachment = device.createTexture({
    size: [1, 1],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  const passPipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module: device.createShaderModule({ code: triangleShader }), entryPoint: "vs" },
    fragment: {
      module: device.createShaderModule({ code: triangleShader }),
      entryPoint: "fs",
      targets: [{ format: "rgba8unorm" }],
    },
  });
  async function drawCount(draws: number, bundles?: GPURenderBundle[]) {
    device.pushErrorScope("validation");
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: attachment.createView(),
        loadOp: "clear",
        storeOp: "store",
      }],
      maxDrawCount: 1,
    });
    pass.setPipeline(passPipeline);
    for (let index = 0; index < draws; index += 1) {
      pass.draw(3);
    }
    if (bundles) {
      pass.executeBundles(bundles);
    }
    pass.end();
    device.queue.submit([encoder.finish()]);
    return device.popErrorScope();
  }
  assert(await drawCount(1) === null, "one draw");
  assert((await drawCount(2)) instanceof GPUValidationError, "two draws");
  const bundleEncoder = device.createRenderBundleEncoder({
    colorFormats: ["rgba8unorm"],
  });
  assert(throwName(() => bundleEncoder.pushDebugGroup("group")) === "NotSupportedError", "push");
  assert(throwName(() => bundleEncoder.popDebugGroup()) === "NotSupportedError", "pop");
  assert(throwName(() => bundleEncoder.insertDebugMarker("mark")) === "NotSupportedError", "marker");
  assert(
    throwName(() => bundleEncoder.setViewport(0, 0, 1, 1, 0, 1)) === "NotSupportedError",
    "viewport",
  );
  assert(throwName(() => bundleEncoder.setScissorRect(0, 0, 1, 1)) === "NotSupportedError", "scissor");
  assert(throwName(() => bundleEncoder.setViewport(NaN, 0, 1, 1, 0, 1)) === "TypeError", "viewport nan");
  bundleEncoder.setPipeline(passPipeline);
  bundleEncoder.draw(3);
  const bundle = bundleEncoder.finish();
  assert(throwName(() => bundleEncoder.pushDebugGroup("group")) === "InvalidStateError", "finished");
  assert(throwName(() => bundleEncoder.setViewport(NaN, 0, 1, 1, 0, 1)) === "TypeError", "finished nan");
  assert(
    throwName(() => bundleEncoder.setViewport(0, 0, 1, 1, 0, 1)) === "InvalidStateError",
    "finished viewport",
  );
  assert(await drawCount(0, [bundle]) === null, "one bundle");
  assert((await drawCount(0, [bundle, bundle])) instanceof GPUValidationError, "two bundles");
  device.destroy();
});
