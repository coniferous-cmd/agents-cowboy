#!/usr/bin/env node

const { existsSync } = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..");
const platforms = [
  ["darwin-x64", "cowboy"],
  ["darwin-arm64", "cowboy"],
  ["linux-x64", "cowboy"],
  ["linux-arm64", "cowboy"],
  ["win32-x64", "cowboy.exe"],
  ["win32-arm64", "cowboy.exe"]
];

const missing = platforms
  .map(([platform, executable]) => path.join(root, "dist", platform, executable))
  .filter((binary) => !existsSync(binary));

if (missing.length > 0) {
  console.error("Missing npm release binaries:");
  for (const binary of missing) {
    console.error(`  ${binary}`);
  }
  process.exit(1);
}
