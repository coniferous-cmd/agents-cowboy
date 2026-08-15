#!/usr/bin/env node

const { spawn } = require("node:child_process");
const { existsSync } = require("node:fs");
const path = require("node:path");

const supportedPlatforms = new Set([
  "darwin-x64",
  "darwin-arm64",
  "linux-x64",
  "linux-arm64",
  "win32-x64",
  "win32-arm64"
]);

const platform = `${process.platform}-${process.arch}`;

if (!supportedPlatforms.has(platform)) {
  console.error(`cowboy does not ship a binary for ${platform}.`);
  process.exit(1);
}

const executable = process.platform === "win32" ? "cowboy.exe" : "cowboy";
const binaryPath = path.join(__dirname, "..", "dist", platform, executable);

if (!existsSync(binaryPath)) {
  console.error(`cowboy binary not found at ${binaryPath}`);
  console.error("Reinstall the package or publish it from a complete multi-platform release build.");
  process.exit(1);
}

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit"
});

child.on("error", (error) => {
  console.error(error.message);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
