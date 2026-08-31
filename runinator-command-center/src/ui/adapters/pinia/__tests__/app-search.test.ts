import { createPinia, setActivePinia } from "pinia";
import { describe, expect, it } from "vitest";
import { useAppStore } from "../app";

describe("app search state", () => {
  it("normalizes search reactively as the query changes", () => {
    setActivePinia(createPinia());
    const app = useAppStore();

    app.searchQuery = "";
    expect(app.normalizedSearch).toBe("");

    app.searchQuery = "  Request Headers  ";
    expect(app.normalizedSearch).toBe("request headers");
  });
});
