// Buitenzorg SDK for VS Code (requirements.md §13.1). Skeleton implementation:
// scaffolding, build & run in QEMU, and manifest validation. The build/run
// commands shell out to the repo scripts (scripts/build-hello-csharp + bzimage
// --run), matching the pipeline used to build xox.elf.
//
// This is a working skeleton; DAP-based breakpoint debugging + hot-reload
// (§13.1) plug into the `buitenzorg` debug type and are the next step.

import * as vscode from "vscode";
import * as cp from "child_process";
import * as path from "path";

function repoRoot(): string {
  const ws = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
  return ws;
}

function run(cmd: string, cwd: string, out: vscode.OutputChannel): void {
  out.show(true);
  out.appendLine(`$ ${cmd}`);
  const child = cp.spawn(cmd, { cwd, shell: true });
  child.stdout.on("data", (d) => out.append(d.toString()));
  child.stderr.on("data", (d) => out.append(d.toString()));
  child.on("close", (code) => out.appendLine(`\n[exit ${code}]`));
}

export function activate(context: vscode.ExtensionContext): void {
  const out = vscode.window.createOutputChannel("Buitenzorg");

  context.subscriptions.push(
    vscode.commands.registerCommand("buitenzorg.newProject", async () => {
      const name = await vscode.window.showInputBox({ prompt: "App name" });
      if (!name) return;
      run(`dotnet run --project sdk/bz -- new desktop-csharp ${name}`, repoRoot(), out);
    }),

    vscode.commands.registerCommand("buitenzorg.buildRun", () => {
      const root = repoRoot();
      // Build the C# apps + kernel image, then boot it in QEMU.
      run(
        `pwsh -File scripts/build-hello-csharp.ps1; ` +
          `cd kernel; cargo run --release -p bzimage -- --run`,
        root,
        out
      );
    }),

    vscode.commands.registerCommand("buitenzorg.validateManifest", () => {
      const doc = vscode.window.activeTextEditor?.document;
      const manifest = doc?.fileName ?? path.join(repoRoot(), "app.manifest");
      run(`dotnet run --project sdk/bz -- manifest validate "${manifest}"`, repoRoot(), out);
    })
  );
}

export function deactivate(): void {}
