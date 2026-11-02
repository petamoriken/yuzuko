console.log(JSON.stringify(Yuzuko.args));
try {
  (Yuzuko.args as unknown as string[]).push("no");
  console.log("pushed");
} catch (error) {
  console.log(error instanceof Error ? error.name : "thrown");
}
