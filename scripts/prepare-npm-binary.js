#!/usr/bin/env node

const { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync } = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const root = path.join(__dirname, "..");
const executable = process.platform === "win32" ? "cowboy.exe" : "cowboy";
const platform = `${process.platform}-${process.arch}`;
const dist = path.join(root, "dist");

for (const stale of ["cowboy", "cowboy.exe"]) {
  rmSync(path.join(dist, stale), { force: true });
}

if (process.env.COWBOY_SKIP_BUILD === "1") {
  const binary = path.join(dist, platform, executable);

  if (!existsSync(binary)) {
    console.error(`Expected prebuilt binary at ${binary}`);
    process.exit(1);
  }

  process.exit(0);
}

const build = spawnSync("cargo", ["build", "--release"], {
  cwd: root,
  stdio: "inherit"
});

if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

const source = path.join(root, "target", "release", executable);
const destinationDir = path.join(dist, platform);
const destination = path.join(destinationDir, executable);

mkdirSync(destinationDir, { recursive: true });
copyFileSync(source, destination);

if (process.platform !== "win32") {
  chmodSync(destination, 0o755);
}
