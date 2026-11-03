console.log("URL");
console.log(new URL("https://example.com/a"));

const extra = new URL("https://example.com/a");
extra.extra = 1;
console.log("extra");
console.log(extra);

console.log("nested");
console.log({ a: { b: { c: { d: new URL("https://example.com/a") } } } });

const headers = new Headers();
headers.append("z", "1");
headers.append("b", "1");
headers.append("b", "3");
headers.append("a", "3");
headers.append("m", "2");
console.log("headers");
console.log(headers);

console.log("typed-headers");
console.log(new Headers({ "content-type": "text/plain", "x-a": "1" }));
console.log(new Headers());

console.log("params");
console.log(new URLSearchParams("a=1&b=2"));
console.log(new URLSearchParams("a=1&b=2&c=3&d=4&e=5&f=6&g=7&h=8"));
console.log(new URLSearchParams("z=1&a=2"));
console.log(new URLSearchParams());

let calls = 0;
class MyURL extends URL {}
const mine = new MyURL("https://example.com/a");
for (const name of [
  "Deno.customInspect",
  "Deno.privateCustomInspect",
  "nodejs.util.inspect.custom",
]) {
  Object.defineProperty(mine, Symbol.for(name), {
    value() {
      calls += 1;
      return "hook";
    },
  });
  Object.defineProperty(MyURL.prototype, Symbol.for(name), {
    value() {
      calls += 1;
      return "hook";
    },
  });
}
console.log("my-url");
console.log(mine);

class MyHeaders extends Headers {}
const myHeaders = new MyHeaders();
myHeaders.append("a", "1");
console.log("my-headers");
console.log(myHeaders);

class MyParams extends URLSearchParams {}
console.log("my-params");
console.log(new MyParams("a=1"));
console.log("calls");
console.log(calls);
