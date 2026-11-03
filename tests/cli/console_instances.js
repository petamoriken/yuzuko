console.log("MARK controller");
console.log(new AbortController());

const stopped = new AbortController();
stopped.abort("stop");
console.log("MARK stopped");
console.log(stopped);

class MyController extends AbortController {
  get more() {
    return 7;
  }
  extra = 2;
}
const mine = new MyController();
mine.extra = 1;
console.log("MARK subclass");
console.log(mine);

const cycle = new AbortController();
cycle.abort(cycle);
console.log("MARK cycle");
console.log(cycle);

console.log("MARK prototype");
console.log(AbortController.prototype);

console.log("MARK unbound");
console.log(Object.create(AbortController.prototype));

console.log("MARK event");
console.log(new Event("go", { bubbles: true }));

console.log("MARK custom");
console.log(new CustomEvent("go", { detail: "x" }));

class MyEvent extends Event {
  get more() {
    return 3;
  }
  extra = 4;
}
console.log("MARK myevent");
console.log(new MyEvent("go"));

console.log("MARK message");
console.log(
  new MessageEvent("message", {
    data: "hi",
    origin: "https://example.com",
    lastEventId: "1",
  }),
);

console.log("MARK close");
console.log(new CloseEvent("close", { wasClean: true, code: 1000, reason: "bye" }));

console.log("MARK pointer");
console.log(new PointerEvent("move", { clientX: 4, pointerId: 7, bubbles: true }));

console.log("MARK blob");
console.log(new Blob(["hi"], { type: "text/plain" }));

console.log("MARK file");
console.log(new File(["hi"], "a.txt", { type: "text/plain" }));

class MyFile extends File {
  get more() {
    return 5;
  }
  extra = 9;
}
console.log("MARK myfile");
console.log(new MyFile(["ab"], "a.txt", { type: "text/plain" }));

console.log("MARK encoder");
console.log(new TextEncoder());

console.log("MARK decoder");
console.log(new TextDecoder());

console.log("MARK request");
console.log(new Request("https://example.com"));

console.log("MARK response");
console.log(new Response("hi"));

const target = new EventTarget();
target.a = 1;
console.log("MARK target");
console.log(target);

console.log("MARK performance");
console.log(performance);

console.log("MARK channel");
console.log(new MessageChannel());

console.log("MARK reader");
console.log(new FileReader());

console.log("MARK point");
console.log(new DOMPoint(1, 2, 3, 4));

console.log("MARK exception");
console.log(new DOMException("boom", "NotFoundError"));

const shadowed = new AbortController();
let reads = 0;
Object.defineProperty(shadowed, "signal", {
  configurable: true,
  get() {
    reads++;
    return 1;
  },
});
console.log("MARK shadow");
console.log(shadowed);
console.log(reads);

let hooks = 0;
const hooked = new AbortController();
for (const name of [
  "Deno.customInspect",
  "Deno.privateCustomInspect",
  "nodejs.util.inspect.custom",
]) {
  Object.defineProperty(hooked, Symbol.for(name), {
    value() {
      hooks++;
      return "hook";
    },
  });
}
console.log("MARK hook");
console.log(hooked);
console.log(hooks);

console.log("MARK deep");
console.log({ a: { b: { c: { d: { e: new AbortController() } } } } });

console.log("MARK deeper");
console.log({ a: { b: { c: { d: { e: { f: new AbortController() } } } } } });
