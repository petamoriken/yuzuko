import assert from "node:assert/strict";
import test from "node:test";
import { API } from "typescript/sync";
import { createFileSystem } from "typescript/fs";
import { errorDiagnostics, errorsFromProgram, formatDiagnostic, isSkipped, rootFilesForCheck } from "./check.mjs";

const api = new API();
process.once("exit", () => {
  try {
    api[Symbol.dispose]();
  } catch {
    // The compiler process has already exited.
  }
});

test("skips the syntax fixtures and keeps every other root", () => {
  const roots = [
    "/repo/tests/defer/invalid.json",
    "/repo/tests/source/bad.ts",
    "/repo/tests/window_test.ts",
    "/repo/bad.ts",
  ];
  assert.equal(isSkipped(roots[0]), true);
  assert.equal(isSkipped(roots[1]), true);
  assert.equal(isSkipped(roots[2]), false);
  assert.equal(isSkipped(roots[3]), false);
  assert.deepEqual(rootFilesForCheck(roots), ["/repo/tests/window_test.ts", "/repo/bad.ts"]);
});

test("formats a diagnostic with a 1-based position", () => {
  const text = formatDiagnostic(
    {
      fileName: "/repo/tests/bad.ts",
      startPosition: { line: 3, character: 6 },
      code: 2322,
      text: "Type 'string' is not assignable to type 'number'.",
      relatedInformation: [
        {
          fileName: "/repo/tests/bad.ts",
          startPosition: { line: 0, character: 0 },
          code: 2322,
          text: "The expected type comes from this declaration.",
        },
      ],
    },
    "/repo",
  );
  assert.equal(
    text,
    [
      "tests/bad.ts(4,7): error TS2322: Type 'string' is not assignable to type 'number'.",
      "  tests/bad.ts(1,1): error TS2322: The expected type comes from this declaration.",
    ].join("\n"),
  );
});

test("drops fixture diagnostics and keeps a type error in another file", () => {
  const diagnostics = errorDiagnostics([
    { category: 1, fileName: "/repo/tests/defer/invalid.json", code: 1005, text: "'}' expected." },
    { category: 1, fileName: "/repo/tests/typed.ts", code: 2322, text: "Type 'string' is not assignable to type 'number'." },
    { category: 0, fileName: "/repo/tests/typed.ts", code: 6133, text: "unused" },
  ]);
  assert.deepEqual(
    diagnostics.map((diagnostic) => diagnostic.code),
    [2322],
  );
});

test("reports a type error after the fixtures leave the root list", () => {
  const files = [
    ["/repo/good.ts", "export const value: number = 1;\n"],
    ["/repo/bad.ts", "export const value: number = \"no\";\n"],
    ["/repo/tests/defer/invalid.json", "{\n"],
    ["/repo/tests/source/bad.ts", "export const x = {\n"],
  ];
  const snapshot = api.createSnapshot({
    fileSystem: createFileSystem(files),
    createPrograms: [
      {
        rootFiles: rootFilesForCheck(files.map(([fileName]) => fileName)),
        // Enum options are numbers. A string such as "ESNext" panics the compiler.
        compilerOptions: {
          strict: true,
          noEmit: true,
          noLib: true,
          noUnusedLocals: true,
        },
      },
    ],
  });
  try {
    const program = snapshot.operation.createdPrograms?.[0];
    assert.ok(program);
    const errors = errorsFromProgram(program);
    assert.equal(
      errors.some((diagnostic) => diagnostic.code === 2322 && diagnostic.fileName?.endsWith("/bad.ts")),
      true,
    );
    assert.equal(
      errors.some((diagnostic) => isSkipped(diagnostic.fileName ?? "")),
      false,
    );
    assert.equal(
      errors.some((diagnostic) => diagnostic.fileName?.endsWith("/good.ts")),
      false,
    );
  } finally {
    snapshot[Symbol.dispose]();
  }
});

test("reports a type error in a file that uses import source", () => {
  const files = [
    [
      "/repo/phase.ts",
      "import source mod from \"./mod.ts\";\nexport const value: number = \"no\";\nvoid mod;\n",
    ],
    ["/repo/mod.ts", "export const value = 1;\n"],
  ];
  const snapshot = api.createSnapshot({
    fileSystem: createFileSystem(files),
    createPrograms: [
      {
        rootFiles: ["/repo/phase.ts"],
        // Enum options are numbers. A string such as "ESNext" panics the compiler.
        compilerOptions: {
          strict: true,
          noEmit: true,
          noLib: true,
          noUnusedLocals: true,
          target: 99,
          module: 99,
          moduleDetection: 3,
          moduleResolution: 100,
          allowImportingTsExtensions: true,
        },
      },
    ],
  });
  try {
    const program = snapshot.operation.createdPrograms?.[0];
    assert.ok(program);
    const errors = errorsFromProgram(program);
    assert.equal(
      errors.some(
        (diagnostic) => diagnostic.code === 2322 && diagnostic.fileName?.endsWith("/phase.ts"),
      ),
      true,
    );
    assert.equal(
      errors.some((diagnostic) => diagnostic.fileName?.endsWith("/phase.ts") && diagnostic.code === 1005),
      false,
    );
  } finally {
    snapshot[Symbol.dispose]();
  }
});
