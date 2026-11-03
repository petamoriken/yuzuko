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

Yuzuko.test("FontFaceSet iterators follow insertion order", async () => {
  assert(Yuzuko.fonts instanceof EventTarget, "event target");
  let illegal = false;
  try {
    new FontFaceSet();
  } catch (error) {
    illegal = error instanceof TypeError;
  }
  assert(illegal, "illegal constructor");

  const bytes = await (await fetch(new URL("YuzukoTest.ttf", fonts))).arrayBuffer();
  async function face(family: string): Promise<FontFace> {
    const loaded = await new FontFace(family, bytes.slice(0)).load();
    assert(Yuzuko.fonts.add(loaded) === Yuzuko.fonts, "add returns the set");
    return loaded;
  }

  const before = Yuzuko.fonts.size;
  const first = await face("YuzukoSetA");
  const second = await face("YuzukoSetB");
  const third = await face("YuzukoSetC");
  assert(Yuzuko.fonts.add(first) === Yuzuko.fonts, "duplicate add");
  assert(Yuzuko.fonts.size === before + 3, `size ${Yuzuko.fonts.size}`);
  assert(Yuzuko.fonts.has(second), "has");
  const foreign = {} as FontFace;
  assert(Yuzuko.fonts.has(foreign) === false, "has foreign");
  assert(Yuzuko.fonts.delete(foreign) === false, "delete foreign");
  assert(Yuzuko.fonts.status === "loaded", Yuzuko.fonts.status);

  const tail = [...Yuzuko.fonts].slice(-3);
  assert(tail[0] === first && tail[1] === second && tail[2] === third, "order");
  const keys = [...Yuzuko.fonts.keys()].slice(-3);
  const values = [...Yuzuko.fonts.values()].slice(-3);
  assert(keys[0] === first && values[2] === third, "keys and values");
  assert(Yuzuko.fonts[Symbol.iterator] === Yuzuko.fonts.values, "iterator");
  const pairs = [...Yuzuko.fonts.entries()].slice(-3);
  assert(pairs.every(([left, right]) => left === right), "entries");

  const receiver = { tag: "set" };
  let seen = 0;
  Yuzuko.fonts.forEach(function (value, key, parent) {
    if (value === first) {
      seen += 1;
      assert(this === receiver, "thisArg");
      assert(value === key, "key");
      assert(parent === Yuzuko.fonts, "parent");
    }
  }, receiver);
  assert(seen === 1, "forEach");

  let rethrown = false;
  try {
    Yuzuko.fonts.forEach(() => {
      throw new TypeError("boom");
    });
  } catch (error) {
    rethrown = error instanceof TypeError && error.message === "boom";
  }
  assert(rethrown, "forEach throw");
  let bad = false;
  try {
    Reflect.apply(Yuzuko.fonts.forEach, Yuzuko.fonts, ["no"]);
  } catch (error) {
    bad = error instanceof TypeError;
  }
  assert(bad, "forEach function");

  const visited: FontFace[] = [];
  const extra = await new FontFace("YuzukoSetD", bytes.slice(0)).load();
  for (const item of Yuzuko.fonts) {
    if (item === first) {
      assert(Yuzuko.fonts.delete(second), "delete");
      assert(Yuzuko.fonts.add(extra) === Yuzuko.fonts, "append");
    }
    if (item === first || item === second || item === third || item === extra) {
      visited.push(item);
    }
  }
  assert(
    visited.length === 3 &&
      visited[0] === first &&
      visited[1] === third &&
      visited[2] === extra,
    visited.map((item) => item.family).join(),
  );
  assert(Yuzuko.fonts.has(second) === false, "deleted");
  assert(Yuzuko.fonts.delete(first), "cleanup first");
  assert(Yuzuko.fonts.delete(third), "cleanup third");
  assert(Yuzuko.fonts.delete(extra), "cleanup extra");
  assert(Yuzuko.fonts.size === before, `restored ${Yuzuko.fonts.size}`);
});
