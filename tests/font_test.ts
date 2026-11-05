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

Yuzuko.test("FontFaceSet check, load, ready, and loading events", async () => {
  const fonts = Yuzuko.fonts;
  const readyBefore = fonts.ready;
  await readyBefore;
  assert(fonts.ready === readyBefore, "ready stays while idle");
  assert(fonts.status === "loaded", fonts.status);
  assert(fonts.check("16px sans-serif") === true, "generic");
  assert(fonts.check("16px NotARealFamily") === true, "unknown family");
  assert(fonts.check("caption") === true, "system font");
  const caption = await fonts.load("caption");
  assert(caption.length === 0, "system load");
  const generic = await fonts.load("16px monospace");
  assert(generic.length === 0, "generic load");

  let syntax = false;
  try {
    fonts.check("inherit");
  } catch (error) {
    syntax = error instanceof DOMException && error.name === "SyntaxError";
  }
  assert(syntax, "check syntax");
  let rejected = false;
  try {
    await fonts.load("revert");
  } catch (error) {
    rejected = error instanceof DOMException && error.name === "SyntaxError";
  }
  assert(rejected, "load syntax");

  const bytes = await (await fetch(new URL("YuzukoTest.ttf", new URL("./fonts/", import.meta.url)))).arrayBuffer();
  const face = new FontFace("YuzukoLoad", bytes.slice(0));
  fonts.add(face);
  assert(fonts.check("16px YuzukoLoad") === false, "unloaded");
  assert(fonts.check("16px YuzukoLoad", "B") === false, "unloaded text");

  const order: string[] = [];
  const onLoading = () => order.push("loading");
  const onDone = (event: FontFaceSetLoadEvent) => {
    order.push(`done:${event.fontfaces.length}`);
  };
  const onError = (event: FontFaceSetLoadEvent) => {
    order.push(`error:${event.fontfaces.length}`);
  };
  fonts.addEventListener("loading", onLoading);
  fonts.addEventListener("loadingdone", onDone);
  fonts.addEventListener("loadingerror", onError);
  let attribute = 0;
  fonts.onloading = () => {
    attribute += 1;
  };

  const loaded = await fonts.load("16px YuzukoLoad");
  assert(loaded.length === 1 && loaded[0] === face, "load face");
  assert(face.status === "loaded", face.status);
  assert(fonts.check("16px YuzukoLoad") === true, "loaded check");
  assert(fonts.status === "loaded", "status after load");
  assert(fonts.ready !== readyBefore, "ready replaced");
  await fonts.ready;
  assert(order[0] === "loading" && order[1] === "done:1", order.join(" "));
  assert(attribute === 1, `onloading ${attribute}`);

  const ranged = new FontFace("YuzukoRange", bytes.slice(0), { unicodeRange: "U+0041" });
  fonts.add(ranged);
  assert(fonts.check("16px YuzukoRange", "A") === false, "range pending");
  assert(fonts.check("16px YuzukoRange", "B") === true, "range miss");
  const onlyA = await fonts.load("16px YuzukoRange", "A");
  assert(onlyA.length === 1 && onlyA[0] === ranged, "load A");
  const onlyB = await fonts.load("16px YuzukoRange", "B");
  assert(onlyB.length === 0, "load B");

  const regular = new FontFace("YuzukoPair", bytes.slice(0), { weight: "400" });
  const bold = new FontFace("YuzukoPair", bytes.slice(0), { weight: "700" });
  fonts.add(regular);
  fonts.add(bold);
  const picked = await fonts.load("bold 16px YuzukoPair");
  assert(picked.length === 1 && picked[0] === bold, "bold face");

  const bad = new FontFace("YuzukoBad", new ArrayBuffer(4));
  fonts.add(bad);
  const beforeError = order.length;
  let failed = false;
  try {
    await fonts.load("16px YuzukoBad");
  } catch (error) {
    failed = error instanceof DOMException && error.name === "SyntaxError";
  }
  assert(failed, "bad load");
  assert(order[beforeError] === "loading", order.slice(beforeError).join(" "));
  assert(order.includes("error:1"), order.join(" "));
  assert(fonts.status === "loaded", "status after error");

  fonts.onloading = null;
  fonts.removeEventListener("loading", onLoading);
  fonts.removeEventListener("loadingdone", onDone);
  fonts.removeEventListener("loadingerror", onError);
});

function quotedUrl(file: string): string {
  return `url(${JSON.stringify(new URL(file, fonts).href)})`;
}

async function expectReject(face: FontFace, name: string): Promise<void> {
  assert(face.status === "unloaded", `${face.family} ${face.status}`);
  let rejected: unknown;
  try {
    await face.load();
  } catch (error) {
    rejected = error;
  }
  assert(
    rejected instanceof DOMException && rejected.name === name,
    `${face.family} ${String(rejected)}`,
  );
  assert(face.status === "error", `${face.family} ${face.status}`);
}

async function expectLoad(family: string, source: string): Promise<void> {
  const face = new FontFace(family, source);
  const loaded = await face.load();
  assert(loaded === face && face.status === "loaded", `${family} ${face.status}`);
}

Yuzuko.test("FontFace format() skips an unsupported source and continues", async () => {
  const woff2 = quotedUrl("YuzukoTest.woff2");
  await expectReject(
    new FontFace("YuzukoFmtSkip", `${woff2} format("collection")`),
    "NetworkError",
  );
  await expectLoad(
    "YuzukoFmtNext",
    `url(${JSON.stringify("data:application/octet-stream;base64,bm9wZQ==")}) format("woff2"), ${woff2} format("woff2")`,
  );
  await expectReject(
    new FontFace(
      "YuzukoBadData",
      `url(${JSON.stringify("data:application/octet-stream;base64,bm9wZQ==")})`,
    ),
    "SyntaxError",
  );
  await expectLoad(
    "YuzukoFmtEither",
    `${woff2} format("collection", "woff2")`,
  );
  await expectLoad("YuzukoFmtVar", `${woff2} format("WOFF2-variations")`);
  await expectLoad(
    "YuzukoFmtTech",
    `${woff2} format("woff2") tech(color-COLRv1)`,
  );
  const unclosed = new FontFace("YuzukoSrcBad", "url(");
  await expectReject(unclosed, "SyntaxError");
  await expectReject(new FontFace("YuzukoNotUrl", "not a url"), "NetworkError");
  await expectLoad(
    "YuzukoLocalMiss",
    `local("Yuzuko No Such Font"), ${woff2}`,
  );
});

Yuzuko.test("queryLocalFonts returns sorted FontData", async () => {
  assert(typeof queryLocalFonts === "function", "queryLocalFonts");
  const list = await queryLocalFonts();
  assert(list.length > 0, "installed fonts");
  assert(list[0] instanceof FontData, "FontData");
  for (let index = 1; index < list.length; index += 1) {
    assert(
      list[index - 1].postscriptName <= list[index].postscriptName,
      `${list[index - 1].postscriptName} before ${list[index].postscriptName}`,
    );
  }
  let illegal = false;
  try {
    new FontData();
  } catch (error) {
    illegal = error instanceof TypeError;
  }
  assert(illegal, "illegal constructor");

  const sample = list[0];
  assert(sample.postscriptName.length > 0, "postscriptName");
  assert(sample.family.length > 0, "family");
  assert(typeof sample.fullName === "string", "fullName");
  assert(typeof sample.style === "string", "style");
  const selected = await queryLocalFonts({
    postscriptNames: [sample.postscriptName],
  });
  assert(
    selected.length >= 1 &&
      selected.every((font) => font.postscriptName === sample.postscriptName),
    `filter ${selected.length}`,
  );
  const empty = await queryLocalFonts({ postscriptNames: [] });
  assert(empty.length === 0, "empty filter");
  const unknown = await queryLocalFonts({
    postscriptNames: ["Yuzuko-No-Such-Font"],
  });
  assert(unknown.length === 0, "unknown filter");
  const mixed = list.find(
    (font) => font.postscriptName !== font.postscriptName.toLowerCase(),
  );
  if (mixed) {
    const lower = await queryLocalFonts({
      postscriptNames: [mixed.postscriptName.toLowerCase()],
    });
    assert(lower.length === 0, "case sensitive");
  }
  const again = await queryLocalFonts(null);
  assert(again.length === list.length, "null options");
  const bare = await queryLocalFonts({});
  assert(bare.length === list.length, "empty options");

  let options = false;
  try {
    queryLocalFonts(1 as unknown as QueryOptions);
  } catch (error) {
    options = error instanceof TypeError;
  }
  assert(options, "options type");
  let names = false;
  try {
    queryLocalFonts({ postscriptNames: "Arial" as unknown as string[] });
  } catch (error) {
    names = error instanceof TypeError;
  }
  assert(names, "postscriptNames type");
  let getter = false;
  try {
    queryLocalFonts({
      get postscriptNames(): string[] {
        throw new RangeError("getter");
      },
    });
  } catch (error) {
    getter = error instanceof RangeError;
  }
  assert(getter, "getter");

  const blob = await sample.blob();
  assert(blob instanceof Blob && blob.type === "", `blob ${blob.type}`);
  const bytes = await blob.arrayBuffer();
  assert(bytes.byteLength > 0, "blob bytes");

  await expectLoad("YuzukoLocalPs", `local(${JSON.stringify(sample.postscriptName)})`);
  await expectLoad("YuzukoLocalFamily", `local(${JSON.stringify(sample.family)})`);
  await expectLoad(
    "YuzukoLocalUpper",
    `local(${JSON.stringify(sample.postscriptName.toUpperCase())})`,
  );
});
