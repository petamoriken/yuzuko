Yuzuko.test("text encoder and decoder", () => {
  const encoder = new TextEncoder();
  if (encoder.encoding !== "utf-8") {
    throw new Error(encoder.encoding);
  }
  const bytes = encoder.encode("hi");
  if (bytes.length !== 2 || bytes[0] !== 104 || bytes[1] !== 105) {
    throw new Error("encode");
  }
  const destination = new Uint8Array(1);
  const into = encoder.encodeInto("hi", destination);
  if (into.read !== 1 || into.written !== 1 || destination[0] !== 104) {
    throw new Error("encodeInto");
  }
  const decoder = new TextDecoder("utf-8");
  if (decoder.encoding !== "utf-8") {
    throw new Error("decoder encoding " + decoder.encoding);
  }
  if (decoder.fatal || decoder.ignoreBOM) {
    throw new Error("decoder flags");
  }
  if (decoder.decode(bytes) !== "hi") {
    throw new Error("decode");
  }
  let name = "";
  try {
    new TextDecoder("not-an-encoding");
  } catch (error) {
    name = error.name;
  }
  if (name !== "RangeError") {
    throw new Error(name);
  }
  if (!(encoder instanceof TextEncoder)) {
    throw new Error("instanceof");
  }
});
