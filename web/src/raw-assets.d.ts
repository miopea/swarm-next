declare module "*.css?raw" {
  const source: string;
  export default source;
}

declare module "*.js?raw" {
  const source: string;
  export default source;
}

/**
 * Source read as text, for checks whose subject is the code itself.
 *
 * `geometryInvariant.test.ts` counts the callers of a primitive rather than
 * exercising a path, because an invariant stated at N call sites gets taught to
 * one of them and a behaviour test can only cover the paths somebody thought of.
 */
declare module "*.ts?raw" {
  const source: string;
  export default source;
}
