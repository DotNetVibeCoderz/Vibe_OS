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
// Debugging (type "buitenzorg"): the debug adapter is `bz dap` (sdk/bz/Dap.cs),
// which talks to a running device over bz-link (BZDP, UDP 7070) -- set
// breakpoints in a .js/.ts/.py/.csx file and press F5 (the script is uploaded
// and replayed from a recording: step, step back, variables), or launch a
// native program by name with function breakpoints. Kernel debugging stays
// on QEMU + GDB (buitenzorg.debug).

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

  // The debug adapter: `bz dap` on stdio.
  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("buitenzorg", {
      createDebugAdapterDescriptor(): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
        const bz = findBz();
        out.appendLine(`debug adapter: ${bz.command} ${bz.args.join(" ")}`);
        return new vscode.DebugAdapterExecutable(bz.command, bz.args);
      },
    }),
    // F5 without a launch.json: debug the active file on the device.
    vscode.debug.registerDebugConfigurationProvider("buitenzorg", {
      resolveDebugConfiguration(_folder, config) {
        if (!config.type && !config.request && !config.name) {
          const doc = vscode.window.activeTextEditor?.document;
          if (!doc) return undefined;
          config.type = "buitenzorg";
          config.request = "launch";
          config.name = "Buitenzorg: debug file on device";
          config.program = doc.fileName;
        }
        if (!config.program) {
          vscode.window.showErrorMessage("Buitenzorg: 'program' is required (a script, an ELF, or a program name on the device).");
          return undefined;
        }
        return config;
      },
    })
  );
}

/** `bz dap`: the configured bz, the workspace's build of it, or bz on PATH. */
function findBz(): { command: string; args: string[] } {
  const configured = vscode.workspace.getConfiguration("buitenzorg").get<string>("bzPath") ?? "";
  const candidates = configured
    ? [configured]
    : [path.join(repoRoot(), "dist", "bz", "bz.dll"), path.join(repoRoot(), "sdk", "bz", "bin", "Debug", "net10.0", "bz.dll")];
  for (const c of candidates) {
    if (c.toLowerCase().endsWith(".dll") && fs.existsSync(c)) return { command: "dotnet", args: [c, "dap"] };
    if (configured && fs.existsSync(c)) return { command: c, args: ["dap"] };
  }
  return { command: "bz", args: ["dap"] };
}

export function deactivate(): void {}
