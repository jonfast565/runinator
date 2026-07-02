import { beforeAll } from "vitest";
import { setCommandRuntime } from "../runtime";
import { apiBaseUrl, invokeViaHttp, wsBaseUrl } from "../httpRuntime";

beforeAll(() => {
  setCommandRuntime({
    isTauri: () => false,
    invoke: invokeViaHttp,
    wsBaseUrl,
    apiBaseUrl,
  });
});
