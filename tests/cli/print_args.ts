console.log(JSON.stringify(Yuzuko.args));
try {
  // @ts-expect-error args is a readonly array.
  Yuzuko.args.push("no");
  console.log("pushed");
} catch (error) {
  console.log(error instanceof Error ? error.name : "thrown");
}
