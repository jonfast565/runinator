import { describe, expect, it } from "vitest";
import type { ProviderMetadata } from "../../domain/models";
import { searchProviderCatalog, summarizeProviderCatalog } from "../provider-catalog";

const providers: ProviderMetadata[] = [
  {
    name: "messaging",
    metadata: { credential_scopes: ["chat.write"], contract: "workspace" },
    actions: [
      {
        function_name: "post_message",
        description: "Send a message to a channel",
        parameters: [
          {
            name: "channel",
            description: "Destination room",
            ty: { type: "string" },
            required: true,
            secret: false,
          },
        ],
        results: [
          {
            name: "message_id",
            description: "Identifier of the created message",
            ty: { type: "string" },
          },
        ],
      },
      {
        function_name: "list_channels",
        description: "List available rooms",
        parameters: [],
        results: [],
      },
    ],
  },
  {
    name: "storage",
    metadata: { credential_scopes: ["files.read", "chat.write"], contract: null },
    actions: [
      {
        function_name: "download",
        description: "Download a file",
        parameters: [],
        results: [],
      },
    ],
  },
];

describe("provider catalog search", () => {
  it("returns every action when the provider itself matches", () => {
    const matches = searchProviderCatalog(providers, "MESSAGING");

    expect(matches).toHaveLength(1);
    expect(matches[0]?.actions.map((action) => action.function_name)).toEqual([
      "post_message",
      "list_channels",
    ]);
  });

  it("returns only matching actions for action contract searches", () => {
    expect(searchProviderCatalog(providers, "destination room")[0]?.actions).toHaveLength(1);
    expect(searchProviderCatalog(providers, "created message")[0]?.actions[0]?.function_name).toBe(
      "post_message",
    );
    expect(searchProviderCatalog(providers, "available rooms")[0]?.actions[0]?.function_name).toBe(
      "list_channels",
    );
  });

  it("matches provider scopes and contracts", () => {
    expect(searchProviderCatalog(providers, "workspace")[0]?.provider.name).toBe("messaging");
    expect(searchProviderCatalog(providers, "files.read")[0]?.provider.name).toBe("storage");
  });
});

describe("provider catalog summary", () => {
  it("counts contracts and de-duplicates credential scopes", () => {
    expect(summarizeProviderCatalog(providers)).toEqual({
      providers: 2,
      actions: 3,
      parameters: 1,
      results: 1,
      credentialScopes: 2,
    });
  });
});
