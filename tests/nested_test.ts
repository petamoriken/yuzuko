Yuzuko.test("nested", () => {
  let threw = false;
  try {
    Yuzuko.test("inner", () => {});
  } catch (error) {
    threw = error instanceof TypeError;
  }
  if (!threw) {
    throw new Error("nested test did not throw TypeError");
  }
});
