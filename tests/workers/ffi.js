self.onmessage = (event) => {
  try {
    const lib = Yuzuko.dlopen(event.data.path, {
      yuzuko_add: { parameters: ["i32", "i32"], result: "i32" },
    });
    const value = lib.symbols.yuzuko_add(20, 22);
    lib.close();
    postMessage({ value });
  } catch (error) {
    postMessage({
      error: error instanceof Error ? error.message : String(error),
    });
  }
};
postMessage({ ready: true });
