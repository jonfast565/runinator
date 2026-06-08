import { workspace, ExtensionContext } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration("runinator");
  const serverPath = config.get<string>("serverPath", "runinator-lsp");
  const apiBaseUrl = config.get<string>("apiBaseUrl", "http://127.0.0.1:8080/");
  const autoApply = config.get<boolean>("autoApply", false);
  // apply-on-save target defaults to the metadata base url when serviceUrl is unset.
  const serviceUrl = config.get<string>("serviceUrl", "") || apiBaseUrl;

  // metadata completion reads RUNINATOR_API_BASE_URL from the server's environment.
  const serverEnv = { ...process.env, RUNINATOR_API_BASE_URL: apiBaseUrl };
  const serverOptions: ServerOptions = {
    run: { command: serverPath, transport: TransportKind.stdio, options: { env: serverEnv } },
    debug: { command: serverPath, transport: TransportKind.stdio, options: { env: serverEnv } },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "wdl" }],
    // apply-on-save settings are read by the server from initializationOptions.
    initializationOptions: {
      runinator: { autoApply, serviceUrl },
    },
    synchronize: {
      configurationSection: "runinator",
    },
  };

  client = new LanguageClient(
    "runinatorWdl",
    "Runinator WDL",
    serverOptions,
    clientOptions
  );
  client.start();
  context.subscriptions.push({ dispose: () => client?.stop() });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
