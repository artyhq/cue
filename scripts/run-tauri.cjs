"use strict";

const { spawn } = require("node:child_process");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

if (process.platform === "win32") {
  // vswhom-sys is C++ code. It needs the MSVC compiler and Windows SDK
  // environment, which is not present in a normal PowerShell session.
  const vswhere = path.join(
    process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)",
    "Microsoft Visual Studio",
    "Installer",
    "vswhere.exe",
  );

  if (fs.existsSync(vswhere)) {
    try {
      const installationPath = execFileSync(
        vswhere,
        ["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        { encoding: "utf8" },
      ).trim();
      const devCmd = path.join(installationPath, "Common7", "Tools", "VsDevCmd.bat");

      if (installationPath && fs.existsSync(devCmd)) {
        const environment = execFileSync(
          process.env.ComSpec || "cmd.exe",
          ["/d", "/c", `call "${devCmd}" -arch=x64 -host_arch=x64 && set`],
          { encoding: "utf8", env: process.env, windowsVerbatimArguments: true },
        );

        for (const line of environment.split(/\r?\n/)) {
          const separator = line.indexOf("=");
          if (separator > 0) {
            process.env[line.slice(0, separator)] = line.slice(separator + 1);
          }
        }
      }
    } catch (error) {
      console.warn("Could not initialize the Visual Studio build environment.");
    }
  }

  // A user-level Clang setting makes cc-rs bypass the MSVC environment.
  delete process.env.CXX;
  delete process.env.CC;
  delete process.env.HOST_CXX;
  delete process.env.HOST_CC;
}

const tauriJs = path.join(
  __dirname,
  "..",
  "node_modules",
  "@tauri-apps",
  "cli",
  "tauri.js",
);

const args = process.argv.slice(2);
if (args[0] === "build" && !args.includes("--features")) {
  args.push("--features", "custom-protocol");
}

const child = spawn(process.execPath, [tauriJs, ...args], {
  stdio: "inherit",
  env: process.env,
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
