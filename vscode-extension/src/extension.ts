import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient;

function getBinaryPath(context: vscode.ExtensionContext): string {
  const ext = process.platform === "win32" ? ".exe" : "";
  const bundled = path.join(context.extensionPath, "bin", `pojoc-lsp${ext}`);
  if (fs.existsSync(bundled)) {
    return bundled;
  }
  const config = vscode.workspace.getConfiguration("pojoc");
  return config.get<string>("serverPath", "pojoc-lsp");
}

export function activate(context: vscode.ExtensionContext) {
  const serverPath = getBinaryPath(context);

  const serverOptions: ServerOptions = {
    command: serverPath,
    transport: TransportKind.stdio,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "pojoc" }],
  };

  client = new LanguageClient(
    "pojocLsp",
    "Pojoc Language Server",
    serverOptions,
    clientOptions,
  );

  context.subscriptions.push(client);
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}