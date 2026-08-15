#!/usr/bin/env node

const { readFileSync } = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..");
const releaseWorkflowPath = path.join(root, ".github", "workflows", "release.yml");

const releaseYml = readFileSync(releaseWorkflowPath, "utf8");

const expectedArtifacts = [
  "cowboy-linux-amd64",
  "cowboy-linux-arm64",
  "cowboy-macos-amd64",
  "cowboy-macos-arm64",
  "cowboy-windows-amd64.exe",
];

const unexpectedPattern = /cowboy-/;
const uploadGlobPattern = /files:\s*\|[\s\S]*?cowboy-\*/;

let errors = [];

// Check artifact names in matrix
for (const artifact of expectedArtifacts) {
  const pattern = new RegExp(`artifact:\\s*${artifact.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`);
  if (!pattern.test(releaseYml)) {
    errors.push(`Missing expected artifact name: ${artifact}`);
  }
}

// Check for old cowboy- artifact names (should not exist)
const oldArtifactMatches = releaseYml.match(/artifact:\s*cowboy-/g);
if (oldArtifactMatches) {
  errors.push(`Found old artifact names: ${oldArtifactMatches.join(", ")}`);
}

// Check upload glob uses cowboy pattern
if (!uploadGlobPattern.test(releaseYml)) {
  errors.push("Release upload glob does not use cowboy-* pattern");
}

if (errors.length > 0) {
  console.error("Release workflow contract violations:");
  for (const error of errors) {
    console.error(`  - ${error}`);
  }
  console.error("\nExpected artifacts:", expectedArtifacts.join(", "));
  process.exit(1);
}

console.log("Release workflow contract OK: artifact names and upload glob use cowboy prefix");
