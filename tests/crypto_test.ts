function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

function hex(bytes: BufferSource): string {
  const view = ArrayBuffer.isView(bytes)
    ? new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    : new Uint8Array(bytes);
  return [...view]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function equalBytes(actual: BufferSource, expected: BufferSource): boolean {
  return hex(actual) === hex(expected);
}

async function assertDom(
  run: () => unknown,
  name: string,
): Promise<void> {
  try {
    await run();
  } catch (error) {
    assert(error instanceof DOMException, String(error));
    assert(error.name === name, `${error.name} expected ${name}`);
    return;
  }
  throw new Error(`${name} was not thrown`);
}

function assertTypeError(run: () => unknown): void {
  try {
    run();
  } catch (error) {
    assert(error instanceof TypeError, String(error));
    return;
  }
  throw new Error("TypeError was not thrown");
}

const subtle = crypto.subtle;

Yuzuko.test("SHA-3 and SHAKE match the published empty digests", async () => {
  const empty = new Uint8Array();
  const sha3 = await subtle.digest("SHA3-256", empty);
  assert(
    hex(sha3) ===
      "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a",
    hex(sha3),
  );
  const sha384 = await subtle.digest("SHA3-384", empty);
  assert(
    hex(sha384) ===
      "0c63a75b845e4f7d01107d852e4c2485c51a50aaaa94fc61995e71bbee983a2ac3713831264adb47fb6bd1e058d5f004",
    hex(sha384),
  );
  const sha512 = await subtle.digest("SHA3-512", empty);
  assert(
    hex(sha512) ===
      "a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26",
    hex(sha512),
  );
  const shake128 = await subtle.digest(
    { name: "SHAKE128", outputLength: 256 },
    empty,
  );
  assert(
    hex(shake128) ===
      "7f9c2ba4e88f827d616045507605853ed73b8093f6efbc88eb1a6eacfa66ef26",
    hex(shake128),
  );
  const shake256 = await subtle.digest(
    { name: "SHAKE256", outputLength: 256 },
    empty,
  );
  assert(
    hex(shake256) ===
      "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f",
    hex(shake256),
  );
  await assertDom(
    () => subtle.digest("cSHAKE128", empty),
    "NotSupportedError",
  );
  await assertDom(
    () => subtle.digest("TurboSHAKE128", empty),
    "NotSupportedError",
  );
  await assertDom(
    () => subtle.digest("KangarooTwelve", empty),
    "NotSupportedError",
  );
});

Yuzuko.test("AES-OCB and ChaCha20-Poly1305 round-trip and reject a bad tag", async () => {
  const ocb = await subtle.generateKey(
    { name: "AES-OCB", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
  const iv = new Uint8Array(15).fill(0x22);
  const aad = new Uint8Array([1, 2, 3, 4]);
  const plain = new TextEncoder().encode("ocb-plaintext");
  const sealed = new Uint8Array(
    await subtle.encrypt(
      { name: "AES-OCB", iv, additionalData: aad },
      ocb,
      plain,
    ),
  );
  const opened = new Uint8Array(
    await subtle.decrypt(
      { name: "AES-OCB", iv, additionalData: aad },
      ocb,
      sealed,
    ),
  );
  assert(equalBytes(opened, plain), "AES-OCB plaintext");
  const badOcb = new Uint8Array(sealed);
  badOcb[badOcb.length - 1] ^= 1;
  await assertDom(
    () =>
      subtle.decrypt(
        { name: "AES-OCB", iv, additionalData: aad },
        ocb,
        badOcb,
      ),
    "OperationError",
  );
  await assertDom(
    () => subtle.encrypt({ name: "AES-OCB", iv: new Uint8Array(4) }, ocb, plain),
    "OperationError",
  );

  const chachaKey = new Uint8Array([
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b,
    0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
    0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
  ]);
  const chacha = await subtle.importKey(
    "raw-secret",
    chachaKey,
    "ChaCha20-Poly1305",
    false,
    ["encrypt", "decrypt"],
  );
  const nonce = new Uint8Array([
    0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
  ]);
  const chachaAad = new Uint8Array([
    0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
  ]);
  const message = new TextEncoder().encode(
    "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.",
  );
  const ciphertext = new Uint8Array(
    await subtle.encrypt(
      { name: "ChaCha20-Poly1305", iv: nonce, additionalData: chachaAad },
      chacha,
      message,
    ),
  );
  assert(hex(ciphertext.slice(0, 4)) === "d31a8d34", hex(ciphertext.slice(0, 4)));
  const recovered = new Uint8Array(
    await subtle.decrypt(
      { name: "ChaCha20-Poly1305", iv: nonce, additionalData: chachaAad },
      chacha,
      ciphertext,
    ),
  );
  assert(equalBytes(recovered, message), "ChaCha20-Poly1305 plaintext");
  const badChacha = new Uint8Array(ciphertext);
  badChacha[0] ^= 1;
  await assertDom(
    () =>
      subtle.decrypt(
        { name: "ChaCha20-Poly1305", iv: nonce, additionalData: chachaAad },
        chacha,
        badChacha,
      ),
    "OperationError",
  );
});

Yuzuko.test("KMAC128 matches the NIST sample and verifies", async () => {
  const keyBytes = new Uint8Array(32);
  for (let index = 0; index < keyBytes.length; index += 1) {
    keyBytes[index] = 0x40 + index;
  }
  const key = await subtle.importKey(
    "raw-secret",
    keyBytes,
    "KMAC128",
    false,
    ["sign", "verify"],
  );
  const data = new Uint8Array([0x00, 0x01, 0x02, 0x03]);
  const mac = await subtle.sign(
    { name: "KMAC128", outputLength: 256 },
    key,
    data,
  );
  assert(
    hex(mac) ===
      "e5780b0d3ea6f7d3a429c5706aa43a00fadbd7d49628839e3187243f456ee14e",
    hex(mac),
  );
  assert(
    await subtle.verify(
      { name: "KMAC128", outputLength: 256 },
      key,
      mac,
      data,
    ),
    "KMAC verify",
  );
  const custom = new TextEncoder().encode("My Tagged Application");
  const tagged = await subtle.sign(
    { name: "KMAC128", outputLength: 256, customization: custom },
    key,
    data,
  );
  assert(
    hex(tagged) ===
      "3b1fba963cd8b0b59e8c1a6d71888b7143651af8ba0a7070c0979e2811324aa5",
    hex(tagged),
  );
  assert(
    !(await subtle.verify(
      { name: "KMAC128", outputLength: 512 },
      key,
      mac,
      data,
    )),
    "wrong KMAC length",
  );
});

Yuzuko.test("Argon2id derives a stable secret and an HMAC key", async () => {
  const password = await subtle.importKey(
    "raw-secret",
    new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]),
    "Argon2id",
    false,
    ["deriveBits", "deriveKey"],
  );
  const params = {
    name: "Argon2id",
    memory: 8,
    passes: 1,
    parallelism: 1,
    nonce: new Uint8Array(16).fill(2),
  };
  const first = await subtle.deriveBits(params, password, 256);
  const second = await subtle.deriveBits(params, password, 256);
  assert(first.byteLength === 32, "Argon2 length");
  assert(equalBytes(first, second), "Argon2 is deterministic");
  await assertDom(
    () =>
      subtle.deriveBits(
        { ...params, nonce: new Uint8Array(4) },
        password,
        256,
      ),
    "OperationError",
  );
  const derived = await subtle.deriveKey(
    params,
    password,
    { name: "HMAC", hash: "SHA-256", length: 256 },
    false,
    ["sign", "verify"],
  );
  const message = new Uint8Array([9]);
  const mac = await subtle.sign("HMAC", derived, message);
  assert(await subtle.verify("HMAC", derived, mac, message), "derived HMAC");
});

Yuzuko.test("ML-KEM and a hybrid KEM round-trip and getPublicKey", async () => {
  const kem = await subtle.generateKey(
    "ML-KEM-768",
    true,
    ["encapsulateBits", "encapsulateKey", "decapsulateBits", "decapsulateKey"],
  );
  const encapsulated = await subtle.encapsulateBits("ML-KEM-768", kem.publicKey);
  assert(encapsulated.ciphertext.byteLength === 1088, "ML-KEM ciphertext");
  assert(encapsulated.sharedKey.byteLength === 32, "ML-KEM secret");
  const shared = await subtle.decapsulateBits(
    "ML-KEM-768",
    kem.privateKey,
    encapsulated.ciphertext,
  );
  assert(equalBytes(shared, encapsulated.sharedKey), "ML-KEM decapsulate");
  const publicKey = await subtle.getPublicKey(kem.privateKey, [
    "encapsulateBits",
    "encapsulateKey",
  ]);
  const fromPublic = await subtle.encapsulateKey(
    "ML-KEM-768",
    publicKey,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
  const unwrapped = await subtle.decapsulateKey(
    "ML-KEM-768",
    kem.privateKey,
    fromPublic.ciphertext,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
  const probe = new Uint8Array([1, 2, 3, 4]);
  const iv = new Uint8Array(12).fill(7);
  const box = await subtle.encrypt(
    { name: "AES-GCM", iv },
    fromPublic.sharedKey,
    probe,
  );
  const clear = await subtle.decrypt({ name: "AES-GCM", iv }, unwrapped, box);
  assert(equalBytes(clear, probe), "ML-KEM shared AES-GCM");

  const hybrid = await subtle.generateKey(
    "MLKEM768-X25519",
    true,
    ["encapsulateBits", "decapsulateBits"],
  );
  const hybridBits = await subtle.encapsulateBits(
    "MLKEM768-X25519",
    hybrid.publicKey,
  );
  assert(hybridBits.ciphertext.byteLength === 1120, "hybrid ciphertext");
  const hybridShared = await subtle.decapsulateBits(
    "MLKEM768-X25519",
    hybrid.privateKey,
    hybridBits.ciphertext,
  );
  assert(equalBytes(hybridShared, hybridBits.sharedKey), "hybrid decapsulate");
});

Yuzuko.test("ML-DSA and SLH-DSA-SHA2-128f sign, verify, and getPublicKey", async () => {
  const mldsa = await subtle.generateKey(
    "ML-DSA-44",
    true,
    ["sign", "verify"],
  );
  const message = new TextEncoder().encode("ml-dsa");
  const signature = await subtle.sign("ML-DSA-44", mldsa.privateKey, message);
  assert(
    await subtle.verify("ML-DSA-44", mldsa.publicKey, signature, message),
    "ML-DSA verify",
  );
  const derived = await subtle.getPublicKey(mldsa.privateKey, ["verify"]);
  assert(
    await subtle.verify("ML-DSA-44", derived, signature, message),
    "ML-DSA getPublicKey",
  );
  assert(
    !(await subtle.verify(
      "ML-DSA-44",
      mldsa.publicKey,
      new Uint8Array(8),
      message,
    )),
    "short ML-DSA signature",
  );

  const slh = await subtle.generateKey(
    "SLH-DSA-SHA2-128f",
    true,
    ["sign", "verify"],
  );
  const note = new TextEncoder().encode("slh-dsa");
  const slhSignature = await subtle.sign(
    "SLH-DSA-SHA2-128f",
    slh.privateKey,
    note,
  );
  assert(slhSignature.byteLength === 17088, `SLH signature ${slhSignature.byteLength}`);
  assert(
    await subtle.verify("SLH-DSA-SHA2-128f", slh.publicKey, slhSignature, note),
    "SLH-DSA verify",
  );
  const slhPublic = await subtle.getPublicKey(slh.privateKey, ["verify"]);
  assert(
    await subtle.verify("SLH-DSA-SHA2-128f", slhPublic, slhSignature, note),
    "SLH-DSA getPublicKey",
  );
  await assertDom(
    () => subtle.generateKey("SLH-DSA-SHA2-128s", false, ["sign"]),
    "NotSupportedError",
  );
});

Yuzuko.test("SubtleCrypto.supports accepts implemented suites and rejects the rest", () => {
  assert(SubtleCrypto.supports("digest", "SHA-256") === true, "SHA-256");
  assert(SubtleCrypto.supports("digest", "SHA3-256") === true, "SHA3-256");
  assert(SubtleCrypto.supports("digest", "SHAKE128") === true, "SHAKE128");
  assert(SubtleCrypto.supports("digest", "cSHAKE128") === false, "cSHAKE128");
  assert(
    SubtleCrypto.supports("digest", "no-such-algorithm") === false,
    "unknown",
  );
  assert(SubtleCrypto.supports("encrypt", "AES-OCB") === true, "AES-OCB");
  assert(
    SubtleCrypto.supports("encrypt", {
      name: "ChaCha20-Poly1305",
      iv: new Uint8Array(12),
    }) === true,
    "ChaCha",
  );
  assert(SubtleCrypto.supports("sign", "KMAC128") === true, "KMAC");
  assert(
    SubtleCrypto.supports("deriveBits", "Argon2id", 256) === true,
    "Argon2",
  );
  assert(SubtleCrypto.supports("deriveBits", "Argon2id") === false, "Argon2 length");
  assert(SubtleCrypto.supports("generateKey", "ML-KEM-768") === true, "ML-KEM");
  assert(SubtleCrypto.supports("getPublicKey", "ML-DSA-44") === true, "ML-DSA");
  assert(
    SubtleCrypto.supports("encapsulateBits", "MLKEM768-P256") === true,
    "hybrid",
  );
  assert(
    SubtleCrypto.supports("sign", "SLH-DSA-SHA2-128f") === true,
    "SLH-DSA",
  );
  assert(
    SubtleCrypto.supports("sign", "SLH-DSA-SHA2-128s") === false,
    "other SLH-DSA",
  );
  assert(
    SubtleCrypto.supports("encapsulateKey", "ML-KEM-768", {
      name: "AES-GCM",
      length: 256,
    }) === true,
    "encapsulateKey",
  );
  assertTypeError(() => SubtleCrypto.supports("digest"));
  assertTypeError(() => SubtleCrypto.supports());
});
