// Buitenzorg SDK for VS Code (requirements.md §13.1).
//
// Commands (all shell out to the repo scripts / bz CLI, matching the pipeline
// that builds the suite apps):
//   - buitenzorg.newProject     scaffold via `bz new <template> <name>` (picker)
//   - buitenzorg.buildRun       build C# apps + kernel image, boot in QEMU
//   - buitenzorg.deploy         build image + run the headless smoke test
//   - buitenzorg.debug          boot in QEMU with a GDB server (-s -S) for attach
//   - buitenzorg.validateManifest   `bz manifest validate`
//
// Full DAP breakpoint debugging (mapping VS Code breakpoints to the managed
// app) is still future work; buitenzorg.debug gives kernel-level GDB today.

import * as vscode from "vscode";
import * as cp from "child_process";
import * as path from "path";
import * as fs from "fs";

function repoRoot(): string {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
}

function run(cmd: string, cwd: string, out: vscode.OutputChannel, env?: NodeJS.ProcessEnv): void {
  out.show(true);
  out.appendLine(`$ ${cmd}`);
  const child = cp.spawn(cmd, { cwd, shell: true, env: { ...process.env, ...env } });
  child.stdout.on("data", (d) => out.append(d.toString()));
  child.stderr.on("data", (d) => out.append(d.toString()));
  child.on("close", (code) => out.appendLine(`\n[exit ${code}]`));
}

// PowerShell on Windows, bash elsewhere.
const isWin = process.platform === "win32";
function ps(script: string): string {
  return isWin ? `pwsh -File ${script}` : `bash ${script}`;
}

const TEMPLATES = ["desktop-csharp", "console-csharp", "js-app", "ts-app", "python-app"];

export function activate(context: vscode.ExtensionContext): void {
  const out = vscode.window.createOutputChannel("Buitenzorg");

  context.subscriptions.push(
    // Create a project — pick a template, then a name.
    vscode.commands.registerCommand("buitenzorg.newProject", async () => {
      const template = await vscode.window.showQuickPick(TEMPLATES, {
        title: "Buitenzorg: choose a project template",
        placeHolder: "desktop-csharp",
      });
      if (!template) return;
      const name = await vscode.window.showInputBox({ prompt: "App name" });
      if (!name) return;
      run(`dotnet run --project sdk/bz -- new ${template} ${name}`, repoRoot(), out);
    }),

    // Build & run (dev loop). If the workspace is a template app project (has a
    // build.ps1/build.sh), deploy that app's ELF and boot — so `run myapp` picks
    // it up. Otherwise build every bundled C# app + the image, then boot.
    vscode.commands.registerCommand("buitenzorg.buildRun", () => {
      const root = repoRoot();
      const appBuild = isWin ? "build.ps1" : "build.sh";
      if (fs.existsSync(path.join(root, appBuild))) {
        const cmd = isWin ? `pwsh -File build.ps1 -Run` : `bash build.sh --run`;
        out.appendLine("Detected an app project — building & deploying userapp.elf.");
        out.appendLine("At the OS prompt, launch it with:  run myapp");
        run(cmd, root, out);
      } else {
        run(
          `${ps("scripts/build-hello-csharp.ps1")} ; cd kernel ; cargo run --release -p bzimage -- --run`,
          root,
          out
        );
      }
    }),

    // Deploy = full build + headless smoke test across all boot media.
    vscode.commands.registerCommand("buitenzorg.deploy", () => {
      const root = repoRoot();
      run(`${ps("scripts/build.ps1")} ; ${ps("scripts/smoke-test.ps1")}`, root, out);
    }),

    // Debug the kernel: boot in QEMU with a GDB server, paused (-s -S).
    vscode.commands.registerCommand("buitenzorg.debug", () => {
      const root = repoRoot();
      out.appendLine("QEMU will start paused with a GDB server on :1234.");
      out.appendLine("Attach from a terminal: gdb -> `target remote :1234`");
      out.appendLine("Kernel symbols: kernel/target/x86_64-unknown-none/release/bzkernel");
      run(
        `cd kernel ; cargo run --release -p bzimage -- --run`,
        root,
        out,
        { QEMU_EXTRA: "-s -S" }
      );
    }),

    vscode.commands.registerCommand("buitenzorg.validateManifest", () => {
      const doc = vscode.window.activeTextEditor?.document;
      const manifest = doc?.fileName ?? path.join(repoRoot(), "app.manifest");
      run(`dotnet run --project sdk/bz -- manifest validate "${manifest}"`, repoRoot(), out);
    })
  );

  // Debug config provider: `launch` of type "buitenzorg" maps to the GDB flow.
  context.subscriptions.push(
    vscode.debug.registerDebugConfigurationProvider("buitenzorg", {
      resolveDebugConfiguration(_folder, config) {
        vscode.commands.executeCommand("buitenzorg.debug");
        return undefined; // handled via the QEMU+GDB command (no DAP session yet)
      },
    })
  );
}

export function deactivate(): void {}
