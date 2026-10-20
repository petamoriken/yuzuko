function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const fonts = new URL("./fonts/", import.meta.url);

async function loadBuffer(family: string, file: string): Promise<void> {
  const bytes = await (await fetch(new URL(file, fonts))).arrayBuffer();
  const face = new FontFace(family, bytes);
  const loaded = await face.load();
  assert(loaded.status === "loaded", `${family} status`);
  Yuzuko.fonts.add(face);
}

async function loadUrl(family: string, file: string): Promise<void> {
  const face = new FontFace(family, new URL(file, fonts).href);
  const loaded = await face.load();
  assert(loaded.status === "loaded", `${family} status`);
  Yuzuko.fonts.add(face);
}

function widthOf(family: string): number {
  const canvas = new OffscreenCanvas(80, 40);
  const context = canvas.getContext("2d");
  assert(context, "2d");
  context.font = `20px ${family}`;
  context.fillText("A", 2, 30);
  return context.measureText("A").width;
}

Yuzuko.test("loaded FontFace families shape differently from sans-serif", async () => {
  assert(typeof FontFace === "function", "FontFace");
  assert(Yuzuko.fonts instanceof FontFaceSet, "Yuzuko.fonts");
  assert(globalThis.fonts === undefined, "no globalThis.fonts");
  assert(typeof document === "undefined", "no document");

  await loadBuffer("YuzukoTtfBuf", "YuzukoTest.ttf");
  await loadUrl("YuzukoTtfUrl", "YuzukoTest.ttf");
  await loadBuffer("YuzukoOtfBuf", "YuzukoTest.otf");
  await loadUrl("YuzukoOtfUrl", "YuzukoTest.otf");
  await loadBuffer("YuzukoWoffBuf", "YuzukoTest.woff");
  await loadUrl("YuzukoWoffUrl", "YuzukoTest.woff");
  await loadBuffer("YuzukoWoff2Buf", "YuzukoTest.woff2");
  await loadUrl("YuzukoWoff2Url", "YuzukoTest.woff2");

  const generic = widthOf("sans-serif");
  for (const family of [
    "YuzukoTtfBuf",
    "YuzukoTtfUrl",
    "YuzukoOtfBuf",
    "YuzukoOtfUrl",
    "YuzukoWoffBuf",
    "YuzukoWoffUrl",
    "YuzukoWoff2Buf",
    "YuzukoWoff2Url",
  ]) {
    const width = widthOf(family);
    assert(Math.abs(width - generic) > 1, `${family} width ${width} vs ${generic}`);
  }

  const context = new OffscreenCanvas(40, 20).getContext("2d");
  assert(context, "2d");
  context.font = "16px serif";
  context.fillText("A", 0, 16);
  context.font = "16px monospace";
  context.fillText("A", 0, 16);
  context.font = "16px NotARegisteredFamily";
  assert(context.font.includes("NotARegisteredFamily"), context.font);
  const unloaded = context.measureText("A").width;
  context.font = "16px sans-serif";
  const fallback = context.measureText("A").width;
  assert(unloaded === fallback, `${unloaded} vs ${fallback}`);
});
