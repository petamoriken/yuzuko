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
