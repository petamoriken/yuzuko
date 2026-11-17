/**
 * Editor types for asset imports in examples and tests.
 */

declare module "*.wasm" {
  const mod: any;
  export default mod;
  export const add: any;
  export const answer: any;
  export const quad: any;
  export const run: any;
}

declare module "*.txt" {
  const value: any;
  export default value;
  export { value };
}

declare module "*.bin" {
  const value: any;
  export default value;
  export { value };
}

declare module "*/empty" {
  const value: any;
  export default value;
}
