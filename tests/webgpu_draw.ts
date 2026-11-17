export const triangleShader = `
@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  const positions = array<vec2f, 3>(
    vec2f(0.0, 0.5),
    vec2f(-0.5, -0.5),
    vec2f(0.5, -0.5),
  );
  return vec4f(positions[vertexIndex], 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4f {
  return vec4f(0.0, 1.0, 0.0, 1.0);
}
`;

export const sampleShader = `
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;

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
  return textureSample(tex, samp, vec2f(0.5, 0.5));
}
`;

function channel(pixel: ArrayLike<number>, index: number): number {
  return pixel[index] ?? 0;
}

export function channels(pixel: ArrayLike<number>, format: GPUTextureFormat) {
  if (format === "bgra8unorm" || format === "bgra8unorm-srgb") {
    return {
      r: channel(pixel, 2),
      g: channel(pixel, 1),
      b: channel(pixel, 0),
      a: channel(pixel, 3),
    };
  }
  return {
    r: channel(pixel, 0),
    g: channel(pixel, 1),
    b: channel(pixel, 2),
    a: channel(pixel, 3),
  };
}

export async function readPixel(
  device: GPUDevice,
  texture: GPUTexture,
  x: number,
  y: number,
) {
  const bytesPerRow = 256;
  const buffer = device.createBuffer({
    size: bytesPerRow * texture.height,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });
  const encoder = device.createCommandEncoder();
  encoder.copyTextureToBuffer(
    { texture },
    { buffer, bytesPerRow },
    [texture.width, texture.height],
  );
  device.queue.submit([encoder.finish()]);
  await buffer.mapAsync(GPUMapMode.READ);
  const bytes = new Uint8Array(buffer.getMappedRange());
  const offset = y * bytesPerRow + x * 4;
  const pixel = [
    bytes[offset] ?? 0,
    bytes[offset + 1] ?? 0,
    bytes[offset + 2] ?? 0,
    bytes[offset + 3] ?? 0,
  ];
  buffer.unmap();
  return pixel;
}

export function drawTriangle(
  device: GPUDevice,
  texture: GPUTexture,
  format: GPUTextureFormat,
) {
  const module = device.createShaderModule({ code: triangleShader });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format }],
    },
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: texture.createView(),
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
      loadOp: "clear",
      storeOp: "store",
    }],
  });
  pass.setPipeline(pipeline);
  pass.draw(3);
  pass.end();
  device.queue.submit([encoder.finish()]);
}

export function drawBitmap(
  device: GPUDevice,
  texture: GPUTexture,
  format: GPUTextureFormat,
  bitmap: ImageBitmap,
) {
  const image = device.createTexture({
    size: [bitmap.width, bitmap.height],
    format: "rgba8unorm",
    usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
  });
  device.queue.copyExternalImageToTexture(
    { source: bitmap },
    { texture: image, colorSpace: "srgb" },
    [bitmap.width, bitmap.height],
  );
  const module = device.createShaderModule({ code: sampleShader });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: {
      module,
      entryPoint: "fs",
      targets: [{ format }],
    },
  });
  const sampler = device.createSampler({ magFilter: "nearest", minFilter: "nearest" });
  const group = device.createBindGroup({
    layout: pipeline.getBindGroupLayout(0),
    entries: [
      { binding: 0, resource: sampler },
      { binding: 1, resource: image.createView() },
    ],
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: texture.createView(),
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
}
