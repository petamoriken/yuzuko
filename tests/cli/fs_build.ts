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
console.log(names.map((name) => typeof Yuzuko[name]).join(" "));
