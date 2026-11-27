function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

Yuzuko.test("multipart names escape quotes and line breaks", async () => {
  const form = new FormData();
  form.append('a"\r\nContent-Type: text/evil\r\n\r\nX', "one\rtwo\nthree");
  form.append("f", new File(["z"], 'b"\r\n.txt'));
  const body = await new Request("http://localhost/", {
    method: "POST",
    body: form,
  }).text();
  assert(!body.includes("text/evil\r\n"), body);
  assert(
    body.includes('name="a%22%0D%0AContent-Type: text/evil%0D%0A%0D%0AX"'),
    body,
  );
  assert(body.includes("\r\n\r\none\r\ntwo\r\nthree\r\n"), body);
  assert(body.includes('filename="b%22%0D%0A.txt"'), body);
});

function latin1(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => String.fromCharCode(byte)).join("");
}

Yuzuko.test("formData parses a mixed-case boundary and binary parts", async () => {
  const boundary = "----WebKitFormBoundaryXyZ";
  const head =
    `--${boundary}\r\nContent-Disposition: form-data; name="t"\r\n\r\nhello;world\r\n` +
    `--${boundary}\r\nContent-Disposition: form-data; filename="a;b.bin"; name="f"\r\n` +
    "Content-Type: application/x-bin\r\n\r\n";
  const tail = `\r\n--${boundary}--\r\n`;
  const bytes = Uint8Array.from(
    latin1(new TextEncoder().encode(head)) + "\xff\x00\x80" + tail,
    (char) => char.charCodeAt(0),
  );
  const form = await new Response(bytes, {
    headers: { "content-type": `multipart/form-data; boundary=${boundary}` },
  }).formData();
  const seen: string[] = [];
  for (const [name, value] of form) {
    if (typeof value === "string") {
      seen.push(`${name}=${value}`);
    } else {
      const data = new Uint8Array(await value.arrayBuffer());
      seen.push(`${name}=${value.name}:${value.type}:${[...data].join(",")}`);
    }
  }
  assert(
    seen.join("|") === "t=hello;world|f=a;b.bin:application/x-bin:255,0,128",
    seen.join("|"),
  );
  let thisForm: unknown;
  form.forEach((_value, _name, parent) => {
    thisForm = parent;
  });
  assert(thisForm === form, "forEach passes the form");

  const urlencoded = await new Request("http://localhost/", {
    method: "POST",
    body: "a=1&b=2",
    headers: { "content-type": "application/x-www-form-urlencoded" },
  }).formData();
  const pairs: string[] = [];
  for (const [name, value] of urlencoded) {
    pairs.push(`${name}=${value}`);
  }
  assert(pairs.join("&") === "a=1&b=2", pairs.join("&"));

  let failed = "";
  try {
    await new Response(`--${boundary}\r\nContent-Disposition: form-data; name="n"\r\n\r\nv`, {
      headers: { "content-type": `multipart/form-data; boundary=${boundary}` },
    }).formData();
  } catch (error) {
    failed = (error as Error).name;
  }
  assert(failed === "TypeError", `truncated ${failed}`);
});

Yuzuko.test("FormData and URLSearchParams iterators inherit %IteratorPrototype%", () => {
  const intrinsic = Object.getPrototypeOf(
    Object.getPrototypeOf([][Symbol.iterator]()),
  );
  const form = new FormData();
  form.append("a", "1");
  const params = new URLSearchParams("a=1");
  for (const [label, iterator, tag] of [
    ["FormData", form.entries(), "FormData Iterator"],
    ["URLSearchParams", params.entries(), "URLSearchParams Iterator"],
  ] as const) {
    const prototype = Object.getPrototypeOf(iterator);
    assert(Object.getPrototypeOf(prototype) === intrinsic, `${label} chain`);
    assert(iterator[Symbol.iterator]() === iterator, `${label} @@iterator`);
    assert(Object.prototype.toString.call(iterator) === `[object ${tag}]`, tag);
    assert(!Object.hasOwn(iterator, Symbol.iterator), `${label} own @@iterator`);
  }
  assert([...form.keys()].join() === "a", "FormData keys");
  assert([...params.values()].join() === "1", "URLSearchParams values");
});

Yuzuko.test("urlencoded bodies percent-decode before UTF-8", async () => {
  // A raw lead byte followed by escaped continuation bytes is one character.
  const body = Uint8Array.from([0x61, 0x3d, 0xe3, ...new TextEncoder().encode("%81%82&&b+c=%zz")]);
  const form = await new Response(body, {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  }).formData();
  const entries = [...form].map(([name, value]) => `${name}:${value}`);
  if (entries.join("|") !== "a:\u3042|b c:%zz") {
    throw new Error(JSON.stringify(entries));
  }
});
