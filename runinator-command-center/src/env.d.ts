/// <reference types="vite/client" />

// build stamp injected by vite's `define`; see vite.config.ts and core/utils/build-info.ts.
declare const __APP_VERSION__: string;
declare const __APP_BUILD_ID__: string;
declare const __APP_BUILD_TIME__: string;

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<object, object, unknown>;
  export default component;
}
