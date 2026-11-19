/**
 * Typecheck the root tsconfig program.
 *
 * `tsc -p tsconfig.json` stops after the parse errors in the two syntax
 * fixtures and does not report semantic diagnostics. Those files stay invalid
 * on disk for the runtime tests. This check drops them from the root file list
 * and ignores diagnostics that still name them.
 */
import path from "node:path";
import { DiagnosticCategory } from "typescript/sync";

/**
 * Intentional syntax fixtures. `tests/wpt` is already excluded by tsconfig.
 * Files that use `import source` stay in the program. TypeScript 7.1 parses
 * that form and types the binding as `AbstractModuleSource`.
 */
export const SKIPPED = ["tests/defer/invalid.json", "tests/source/bad.ts"];

/**
 * @param {string} fileName
 */
export function isSkipped(fileName) {
  const normalized = fileName.replaceAll("\\", "/");
  return SKIPPED.some((suffix) => normalized === suffix || normalized.endsWith(`/${suffix}`));
}

/**
 * @param {readonly string[]} fileNames
 */
export function rootFilesForCheck(fileNames) {
  return fileNames.filter((fileName) => !isSkipped(fileName));
}

/**
 * @param {readonly { category: number, fileName?: string }[]} diagnostics
 */
export function errorDiagnostics(diagnostics) {
  return diagnostics.filter(
    (diagnostic) => diagnostic.category === DiagnosticCategory.Error && !isSkipped(diagnostic.fileName ?? ""),
  );
}

/**
 * @param {import("typescript/sync").Program} program
 */
export function errorsFromProgram(program) {
  return errorDiagnostics([
    ...program.getSyntacticDiagnostics(),
    ...program.getBindDiagnostics(),
    ...program.getSemanticDiagnostics(),
  ]);
}

/**
 * @param {import("typescript/sync").API} api
 * @param {string} tsconfigPath
 */
export function checkProject(api, tsconfigPath) {
  const parsed = api.parseConfigFile(tsconfigPath);
  const configErrors = errorDiagnostics(parsed.errors ?? []);
  if (configErrors.length > 0) {
    return { fileCount: 0, errors: configErrors };
  }
  const fileNames = rootFilesForCheck(parsed.fileNames);
  const program = api.createProgram(fileNames, parsed.options);
  return { fileCount: fileNames.length, errors: errorsFromProgram(program) };
}

/**
 * @param {{ fileName?: string, startPosition?: { line: number, character: number }, code: number, text: string, relatedInformation?: readonly { fileName?: string, startPosition?: { line: number, character: number }, code: number, text: string }[] }} diagnostic
 * @param {string} [root]
 */
export function formatDiagnostic(diagnostic, root) {
  const lines = [formatOne(diagnostic, root)];
  for (const related of diagnostic.relatedInformation ?? []) {
    lines.push(`  ${formatOne(related, root)}`);
  }
  return lines.join("\n");
}

/**
 * @param {{ fileName?: string, startPosition?: { line: number, character: number }, code: number, text: string }} diagnostic
 * @param {string} [root]
 */
function formatOne(diagnostic, root) {
  const fileName = diagnostic.fileName;
  const position = diagnostic.startPosition;
  let location = "";
  if (fileName) {
    const relative = root === undefined ? fileName : path.relative(root, fileName);
    const shown = relative.startsWith("..") || path.isAbsolute(relative) ? fileName : relative;
    const line = (position?.line ?? 0) + 1;
    const character = (position?.character ?? 0) + 1;
    location = `${shown}(${line},${character}): `;
  }
  return `${location}error TS${diagnostic.code}: ${diagnostic.text}`;
}
