# @yuzuko/types

TypeScript declarations for the [Yuzuko](https://github.com/petamoriken/yuzuko) runtime. The published file is the `yuzuko.d.ts` from the same version's GitHub release.

```sh
npm install -D @yuzuko/types
```

Include the globals beside the rest of a program:

```ts
/// <reference types="@yuzuko/types" />
```

`compilerOptions.types` also loads them. That list replaces the default `@types` set:

```json
{
  "compilerOptions": {
    "types": ["@yuzuko/types"]
  }
}
```

The file declares the main realm of a default build. It does not reference `lib.dom.d.ts`.
