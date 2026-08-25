/// <reference types="vite/client" />

/**
 * The build stamp compiled into the bundle, so a running page can tell whether
 * it predates the Hive serving it. Set by the packaging scripts; absent under
 * `vite dev` and in tests, where there is no release to be behind.
 */
interface ImportMetaEnv {
  readonly VITE_SWARM_BUILD_VERSION?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
