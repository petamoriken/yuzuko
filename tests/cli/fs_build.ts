const names = [
  "readFile",
  "readFileSync",
  "writeFile",
  "writeTextFile",
  "open",
  "stat",
  "mkdir",
  "remove",
  "FsFile",
  "SeekMode",
  "errors",
  "chmod",
];
console.log(
  names
    .map((name) => {
      // @ts-expect-error Yuzuko has no string index.
      return typeof Yuzuko[name];
    })
    .join(" "),
);
