function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const NOT_BUFFER = "The MP4 data is not a BufferSource";
const DETACHED = "The MP4 data is detached";
const EMPTY = "The MP4 data is empty";
const INVALID = "The MP4 file is not a valid container";

function thrown(run: () => unknown): unknown {
  try {
    run();
  } catch (error) {
    return error;
  }
  throw new Error("expected a throw");
}

function throws(run: () => unknown, name: string, message: string): void {
  const error = thrown(run);
  assert(error instanceof Error, "error");
  assert(error.name === name, error.name);
  assert(error.message === message, error.message);
}

async function progressive(): Promise<Uint8Array> {
  const url = new URL("./fixtures/media/progressive.mp4", import.meta.url);
  return new Uint8Array(await (await fetch(url)).arrayBuffer());
}

function bytes(chunk: EncodedVideoChunk | EncodedAudioChunk): number[] {
  const copy = new Uint8Array(chunk.byteLength);
  chunk.copyTo(copy);
  return [...copy];
}

Yuzuko.test("demuxMp4 reads a progressive MP4 file", async () => {
  assert(Yuzuko.demuxMp4.length === 1, "length");
  const encoded = await progressive();
  const movie = Yuzuko.demuxMp4(encoded);
  assert(movie.video.length === 1, "video tracks");
  assert(movie.audio.length === 1, "audio tracks");

  const video = movie.video[0];
  assert(video.codec === "avc1.42E01E", video.codec);
  assert(video.codedWidth === 16, "width");
  assert(video.codedHeight === 32, "height");
  assert(
    [...video.description].join() === "1,66,224,30,255,225,0,1,103,1,0,1,104",
    "avcC",
  );
  assert(video.chunks.length === 2, "video samples");
  assert(video.chunks[0] instanceof EncodedVideoChunk, "video chunk");
  assert(video.chunks[0].type === "key", "key");
  assert(video.chunks[1].type === "delta", "delta");
  assert(video.chunks[0].timestamp === 0, "video timestamp 0");
  assert(video.chunks[1].timestamp === 40000, "video timestamp 1");
  assert(video.chunks[0].duration === 40000, "video duration 0");
  assert(video.chunks[1].duration === 40000, "video duration 1");
  assert(bytes(video.chunks[0]).join() === "0,0,0,1,101", "key bytes");
  assert(bytes(video.chunks[1]).join() === "0,0,0,1,65", "delta bytes");
  video.description[0] = 0;
  assert(Yuzuko.demuxMp4(encoded).video[0].description[0] === 1, "fresh avcC");

  const audio = movie.audio[0];
  assert(audio.codec === "mp4a.40.2", audio.codec);
  assert(audio.sampleRate === 44100, "rate");
  assert(audio.numberOfChannels === 2, "channels");
  assert([...audio.description].join() === "18,16", "asc");
  assert(audio.chunks.length === 3, "audio samples");
  assert(
    audio.chunks.every(
      (chunk) => chunk instanceof EncodedAudioChunk && chunk.type === "key",
    ),
    "audio keys",
  );
  assert(audio.chunks[0].timestamp === 0, "audio timestamp 0");
  assert(audio.chunks[1].timestamp === 23219, "audio timestamp 1");
  assert(audio.chunks[2].timestamp === 46439, "audio timestamp 2");
  assert(audio.chunks[0].duration === 23219, "audio duration 0");
  assert(audio.chunks[1].duration === 23219, "audio duration 1");
  assert(audio.chunks[2].duration === 23219, "audio duration 2");
  assert(bytes(audio.chunks[0]).join() === "17,34", "aac 0");
  assert(bytes(audio.chunks[1]).join() === "51,68,85", "aac 1");
  assert(bytes(audio.chunks[2]).join() === "102", "aac 2");

  const wrapped = new Uint8Array(encoded.byteLength + 4);
  wrapped.set(encoded, 4);
  const view = new DataView(wrapped.buffer, 4, encoded.byteLength);
  const fromView = Yuzuko.demuxMp4(view);
  assert(fromView.audio[0].chunks[2].timestamp === 46439, "dataview");

  const shared = new SharedArrayBuffer(encoded.byteLength);
  new Uint8Array(shared).set(encoded);
  const fromShared = Yuzuko.demuxMp4(shared);
  assert(fromShared.video[0].chunks.length === 2, "shared");

  throws(() => Yuzuko.demuxMp4(), "TypeError", NOT_BUFFER);
  throws(() => Yuzuko.demuxMp4(null as never), "TypeError", NOT_BUFFER);
  throws(() => Yuzuko.demuxMp4(1 as never), "TypeError", NOT_BUFFER);
  throws(() => Yuzuko.demuxMp4("mp4" as never), "TypeError", NOT_BUFFER);
  const empty = thrown(() => Yuzuko.demuxMp4(new Uint8Array()));
  assert(empty instanceof DOMException, "empty type");
  throws(() => Yuzuko.demuxMp4(new Uint8Array()), "EncodingError", EMPTY);
  throws(
    () => Yuzuko.demuxMp4(new Uint8Array([0, 0, 0, 1])),
    "EncodingError",
    INVALID,
  );
  const copy = encoded.slice();
  structuredClone(copy.buffer, { transfer: [copy.buffer] });
  throws(() => Yuzuko.demuxMp4(copy), "TypeError", DETACHED);
});

Yuzuko.test("a dedicated worker can demux MP4", async () => {
  const worker = new Worker(new URL("./workers/mp4.js", import.meta.url).href);
  const probe = await new Promise<{ demuxMp4: boolean }>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent) => {
      resolve(event.data as { demuxMp4: boolean });
    };
    worker.onerror = (event: ErrorEvent) => {
      reject(new Error(event.message || "worker error"));
    };
  });
  worker.terminate();
  assert(probe.demuxMp4 === true, "worker demuxMp4");
});
