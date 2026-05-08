const vscode = require('vscode');
const path = require('path');

function activate(context) {
  const serverModule = vscode.Uri.file(
    path.join(context.extensionPath, '..', 'glyim')
  );

  const serverOptions = {
    command: 'glyim',
    args: ['lsp']
  };

  const clientOptions = {
    documentSelector: [{ scheme: 'file', language: 'glyim' }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.g')
    }
  };

  const client = new vscode.LanguageClient(
    'glyim',
    'Glyim Language Server',
    serverOptions,
    clientOptions
  );

  client.start();
}

function deactivate() {}

module.exports = { activate, deactivate };
