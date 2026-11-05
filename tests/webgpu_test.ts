import {
  channels,
  drawBitmap,
  drawTriangle,
  readPixel,
  triangleShader,
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
    await adapter.requestDevice({ requiredFeatures: ["not-a-feature"] });
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
    device.createComputePipeline();
  } catch (caught) {
    invalid = caught instanceof TypeError;
  }
  assert(invalid, "compute pipeline descriptor");
  const compute = device.createCommandEncoder().beginComputePass();
  compute.end();
  const canvas = new OffscreenCanvas(4, 4);
  const context = canvas.getContext("webgpu");
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

const computeShader = `
@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(1)
fn main() {
  data[0] = data[0] + 1u;
}
`;

function depthShader(z, color) {
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

async function readBytes(device, source, size) {
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
  function pipeline(z, pixel) {
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
      unsupported = caught.name === "NotSupportedError";
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

function uintPixel(pixel) {
  return (
    pixel[0] +
    pixel[1] * 256 +
    pixel[2] * 65536 +
    pixel[3] * 16777216
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
      unsupported = error.name === "NotSupportedError";
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
          storageTexture: { access: "nope", format: "r32uint" },
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
  const context = canvas.getContext("webgpu");
  assert(context.label === "", "canvas label");
  let threw = false;
  try {
    context.label = "nope";
  } catch (error) {
    threw = error instanceof TypeError;
  }
  assert(threw, "canvas label assignment");
  assert(context.label === "", "canvas label stays empty");
  device.destroy();
});

const alphaBlend = {
  color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" },
  alpha: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" },
};

function colorTexture(device) {
  return device.createTexture({
    size: [16, 16],
    format: "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
}

async function overdraw(device, blend) {
  const color = colorTexture(device);
  const red = device.createShaderModule({
    code: depthShader("0.0", "1.0, 0.0, 0.0, 1.0"),
  });
  const green = device.createShaderModule({
    code: depthShader("0.0", "0.0, 1.0, 0.0, 0.5"),
  });
  function pipeline(module) {
    const target = { format: "rgba8unorm" };
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

async function coveredPixel(device, alpha) {
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
      threw = error.name === "NotSupportedError";
    }
    assert(threw, "unclippedDepth");
    device.destroy();
    return;
  }
  function pipeline(unclipped) {
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
  async function pixel(unclipped) {
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
      threw = error.name === "NotSupportedError";
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
  function pipeline(z, pixel) {
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
    assert(error.name === "NotSupportedError", `external import ${error.name}`);
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
      layoutThrew = layoutError.name === "NotSupportedError";
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
    closed = error.name === "InvalidStateError";
  }
  assert(closed, "closed frame");
  let typeError = false;
  try {
    device.importExternalTexture({ source: {} });
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
  assert(typeof bundle.pushDebugGroup !== "function", "bundle markers");
  assert(typeof bundle.setViewport !== "function", "bundle viewport");
  bundle.finish();
  device.destroy();
});
