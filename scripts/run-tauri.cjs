"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");

if (process.platform === "win32") {
  // User-level CXX=clang++ makes cc-rs skip MSVC. vswhom-sys then fails
  // with `windows.h` not found because Clang has no Windows SDK includes.
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

const child = spawn(process.execPath, [tauriJs, ...process.argv.slice(2)], {
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
