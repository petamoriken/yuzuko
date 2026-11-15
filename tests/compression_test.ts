function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function readAll(stream) {
  const reader = stream.getReader();
  const chunks = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    chunks.push(value);
  }
  const size = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const output = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

Yuzuko.test("brotli compression round-trips", async () => {
  const input = new TextEncoder().encode("yuzuko brotli");
  const source = new ReadableStream({
    start(controller) {
      controller.enqueue(input);
      controller.close();
    },
  });
  const compressed = await readAll(source.pipeThrough(new CompressionStream("brotli")));
  const again = new ReadableStream({
    start(controller) {
      controller.enqueue(compressed);
      controller.close();
    },
  });
  const output = await readAll(again.pipeThrough(new DecompressionStream("brotli")));
  assert(output.length === input.length, `length ${output.length}`);
  for (let index = 0; index < input.length; index += 1) {
    assert(output[index] === input[index], `byte ${index}`);
  }
});
