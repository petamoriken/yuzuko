//! Surfaces that the web-type allowlist used to omit.

use std::time::Duration;

#[test]
fn allowlist_surfaces_match_the_runtime() {
  yuzuko_dom::register();
  yuzuko_winter::register();
  #[cfg(feature = "image")]
  yuzuko_image::register();
  #[cfg(feature = "canvas")]
  yuzuko_canvas::register();
  #[cfg(feature = "codec")]
  yuzuko_codec::register();
  #[cfg(feature = "audio")]
  yuzuko_audio::register();
  let run = yuzuko_core::run_classic_scripts(
    &[SCRIPT.to_string()],
    "done",
    Duration::from_secs(20),
    None,
    None,
  )
  .expect("runtime");
  if let Some(error) = &run.error {
    panic!("{error}");
  }
  let value = run.value.unwrap_or_else(|| "missing done".to_string());
  assert_eq!(value, "ok");
}

const SCRIPT: &str = r#"
(async () => {
  function assert(condition, message) {
    if (!condition) throw new Error(message);
  }
  async function assertRejects(promise, name, text) {
    try {
      await promise;
    } catch (error) {
      assert(error.name === name, error.name + " " + error.message);
      assert(String(error.message).includes(text), error.message);
      return;
    }
    throw new Error("expected " + name + " " + text);
  }

  const target = new EventTarget();
  const seen = [];
  const bubble = new Event("go");
  target.addEventListener("go", () => {
    seen.push("a");
    bubble.cancelBubble = true;
  });
  target.addEventListener("go", () => {
    seen.push("b");
  });
  target.dispatchEvent(bubble);
  assert(seen.join(",") === "a,b", "same target " + seen);
  assert(bubble.cancelBubble === true, "cancelBubble getter");
  bubble.cancelBubble = false;
  assert(bubble.cancelBubble === true, "false does not clear stop");

  if (typeof File === "function") {
    assert(new File([], "a").webkitRelativePath === "", "webkitRelativePath");
  }

  if (typeof UIEvent === "function") {
    assert(new UIEvent("x", { which: 5 }).which === 5, "UIEvent.which");
    assert(new UIEvent("x").which === 0, "UIEvent.which default");
  }

  if (typeof MouseEvent === "function") {
    const mouse = new MouseEvent("click", {
      clientX: 4.8,
      clientY: 9.2,
      button: 2,
      modifierFn: true,
    });
    assert(mouse.x === 4 && mouse.y === 9, "MouseEvent x y");
    assert(mouse.which === 3, "MouseEvent.which " + mouse.which);
    assert(new MouseEvent("click").which === 1, "default which");
    assert(new MouseEvent("click", { button: -1 }).which === 0, "button -1");
    assert(mouse.getModifierState("Fn") === true, "modifierFn");
  }

  if (typeof KeyboardEvent === "function") {
    const key = new KeyboardEvent("keydown", {
      charCode: 65,
      keyCode: 65,
      modifierCapsLock: true,
      modifierFn: true,
    });
    assert(key.charCode === 65 && key.keyCode === 65, "charCode keyCode");
    assert(key.getModifierState("CapsLock") === true, "CapsLock");
    assert(key.getModifierState("Fn") === true, "Fn");
  }

  if (typeof Touch === "function" && typeof TouchEvent === "function") {
    const touch = new Touch({ identifier: 1, target: new EventTarget() });
    const event = new TouchEvent("touchstart", { touches: [touch] });
    const spread = [...event.touches];
    assert(spread.length === 1 && spread[0] === touch, "TouchList iterator");
    let illegal = false;
    try {
      TouchList.prototype[Symbol.iterator].call({});
    } catch (error) {
      illegal = error instanceof TypeError && error.message === "illegal invocation";
    }
    assert(illegal, "TouchList illegal invocation");
  }

  if (typeof DOMRectList === "function") {
    let illegal = false;
    try {
      DOMRectList.prototype[Symbol.iterator].call({});
    } catch (error) {
      illegal = error instanceof TypeError && error.message === "illegal invocation";
    }
    assert(illegal, "DOMRectList illegal invocation");
  }

  if (typeof ImageDecoder === "function") {
    const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    const binary = atob(png);
    const data = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index++) {
      data[index] = binary.charCodeAt(index);
    }
    const decoder = new ImageDecoder({ type: "image/png", data });
    await decoder.tracks.ready;
    const list = decoder.tracks;
    const track = list[0];
    assert(track != null, "image track");
    track.selected = false;
    assert(list[0] === track, "deselected index");
    assert(list.item(0) === track, "item");
    assert(list.item(1) === null, "item past the end");
    assert(list[1] === undefined, "index past the end");
    const spread = [...list];
    assert(spread.length === 1 && spread[0] === track, "ImageTrackList iterator");
    decoder.close();
  }

  if (typeof caches === "object") {
    const cache = await caches.open("surface");
    const kept = new Response("kept");
    await cache.put("https://example.com/kept", kept);
    assert(await kept.text() === "kept", "original body");
    const matchedKept = await cache.match("https://example.com/kept");
    assert(await matchedKept.text() === "kept", "matched body");

    const absent = new Response();
    await cache.put("https://example.com/null", absent);
    const matchedNull = await cache.match("https://example.com/null");
    assert(matchedNull.body === null, "null body");
    await cache.put("https://example.com/blank", new Response(""));
    const matchedBlank = await cache.match("https://example.com/blank");
    assert(matchedBlank.body !== null, "empty body");
    assert(await matchedBlank.text() === "", "empty text");

    await cache.put("https://example.com/a#b", new Response("frag"));
    const fragment = await cache.match("https://example.com/a");
    assert(fragment && await fragment.text() === "frag", "fragment");

    await cache.put("https://example.com/str", new Response("s"));
    assert(await (await cache.match("https://example.com/str")).text() === "s", "string url");

    await cache.put("https://example.com/q?a=1", new Response("q"));
    assert(await cache.match("https://example.com/q?a=2") === undefined, "search");
    const searched = await cache.match("https://example.com/q?a=2", { ignoreSearch: true });
    assert(searched && await searched.text() === "q", "ignoreSearch");
    const post = new Request("https://example.com/q?a=1", { method: "POST" });
    assert(await cache.match(post) === undefined, "post");
    const posted = await cache.match(post, { ignoreMethod: true });
    assert(posted && await posted.text() === "q", "ignoreMethod");

    await cache.put("https://example.com/order-a", new Response("one"));
    await cache.put("https://example.com/order-b", new Response("two"));
    await cache.put("https://example.com/order-a", new Response("three"));
    const keys = await cache.keys();
    const urls = keys.map((request) => request.url);
    const first = urls.indexOf("https://example.com/order-a");
    const second = urls.indexOf("https://example.com/order-b");
    assert(first >= 0 && second === first + 1, "order " + urls);
    assert(await (await cache.match("https://example.com/order-a")).text() === "three", "replace");

    await cache.put("https://example.com/vary", new Response("v", { headers: { Vary: "accept" } }));
    const varied = await cache.match("https://example.com/vary", { ignoreVary: true });
    assert(varied && varied.headers.get("vary") === "accept", "vary");

    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new Uint8Array([104, 105]));
        controller.close();
      },
    });
    const streamed = new Response(stream);
    await cache.put("https://example.com/hi", streamed);
    const matchedStream = await cache.match("https://example.com/hi");
    assert(await streamed.text() === "hi", "stream original");
    assert(await matchedStream.text() === "hi", "stream match");

    await assertRejects(
      cache.put("https://example.com/partial", new Response("x", { status: 206 })),
      "TypeError",
      "206",
    );
    await assertRejects(
      cache.put("https://example.com/star", new Response("x", { headers: { Vary: "*" } })),
      "TypeError",
      "*",
    );
    await assertRejects(
      cache.put("https://example.com/star2", new Response("x", { headers: { Vary: "accept, *" } })),
      "TypeError",
      "*",
    );
    await assertRejects(cache.add("blob:https://example.com/a"), "TypeError", "HTTP URL");
    await assertRejects(cache.addAll(["blob:https://example.com/a"]), "TypeError", "HTTP URL");
    assert(await cache.addAll([]) === undefined, "empty addAll");

    const removed = await cache.delete("https://example.com/kept");
    assert(removed === true, "delete");
    assert(await cache.match("https://example.com/kept") === undefined, "deleted entry");

    await caches.open("order-b");
    await caches.open("order-a");
    const names = await caches.keys();
    const nameB = names.indexOf("order-b");
    const nameA = names.indexOf("order-a");
    assert(nameB >= 0 && nameA === nameB + 1, "storage order " + names);
    assert(await caches.has("order-b") === true, "has");
    await caches.open("named");
    await (await caches.open("named")).put("https://example.com/named", new Response("n"));
    const byName = await caches.match("https://example.com/named", { cacheName: "named" });
    assert(byName && await byName.text() === "n", "cacheName");
    assert(await caches.match("https://example.com/named", { cacheName: "missing" }) === undefined, "missing cacheName");
    const gone = await caches.open("gone");
    assert(await caches.delete("gone") === true, "storage delete");
    assert(await caches.has("gone") === false, "has after delete");
    await assertRejects(
      gone.put("https://example.com/z", new Response("z")),
      "InvalidStateError",
      "The cache is deleted",
    );
  }

  if (typeof AudioContext === "function") {
    const context = new AudioContext({ sinkId: { type: "none" } });
    const stamp = context.getOutputTimestamp();
    assert(stamp.contextTime === context.currentTime, "contextTime");
    assert(typeof stamp.performanceTime === "number", "performanceTime");
    const offline = new OfflineAudioContext(1, 128, 44100);
    offline.createBufferSource().start(0, 1, 0.1);
    let range = false;
    try {
      offline.createBufferSource().start(0, 0, -1);
    } catch (error) {
      range = error instanceof RangeError;
    }
    assert(range, "negative duration");
    if (typeof fetch === "function") {
      const source = `
        registerProcessor("surface", class extends AudioWorkletProcessor {
          static get parameterDescriptors() {
            return [
              { name: "gain", defaultValue: 1 },
              { name: "pan", defaultValue: 0 },
            ];
          }
          process() { return true; }
        });
      `;
      const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
      await context.audioWorklet.addModule(url);
      const node = new AudioWorkletNode(context, "surface");
      const names = [];
      node.parameters.forEach(function (param, name, parent) {
        "use strict";
        if (this !== 7) throw new Error("this " + this);
        if (parent !== node.parameters) throw new Error("parent");
        if (!(param instanceof AudioParam)) throw new Error("param");
        names.push(name);
      }, 7);
      assert(names.join(",") === "gain,pan", "forEach " + names);
      let stopped = false;
      try {
        node.parameters.forEach(() => {
          throw new Error("stop");
        });
      } catch (error) {
        stopped = error instanceof Error && error.message === "stop";
      }
      assert(stopped, "forEach throw");
    }
    await context.close();
  }

  globalThis.done = "ok";
})().catch((error) => {
  globalThis.done = error && error.stack ? String(error.stack) : String(error);
});
"#;
