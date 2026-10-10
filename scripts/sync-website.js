#!/usr/bin/env node
/**
 * sync-website.js
 *
 * Syncs zenv data to the zorl.cloud website repository.
 * Run this script after version bumps to keep the website in sync.
 *
 * Usage:
 *   node scripts/sync-website.js --version 0.2.3
 *   node scripts/sync-website.js --version 0.2.3 --website-path ../zorl
 */

const fs = require("fs");
const path = require("path");
const SEMVER = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*)(\.(0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*))*)?(\+[0-9a-zA-Z-]+(\.[0-9a-zA-Z-]+)*)?$/;

// Parse command line arguments
function parseArgs(args = process.argv.slice(2)) {
  const result = {
    version: null,
    websitePath: null,
  };

  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--version" && args[i + 1]) {
      result.version = args[i + 1];
      i++;
    } else if (args[i] === "--website-path" && args[i + 1]) {
      result.websitePath = args[i + 1];
      i++;
    } else {
      throw new Error(`Unknown or incomplete argument: ${args[i]}`);
    }
  }

  return result;
}

// Read Cargo.toml to get version if not provided
function getVersionFromCargo() {
  const cargoPath = path.join(__dirname, "..", "Cargo.toml");
  if (!fs.existsSync(cargoPath)) {
    return null;
  }

  const content = fs.readFileSync(cargoPath, "utf8");
  const match = content.match(/^version\s*=\s*"([^"]+)"/m);
  return match ? match[1] : null;
}

// Prepare the zenv section only; never overwrite another product's version.
function prepareLlmsTxt(version, websitePath) {
  const llmsPath = path.join(websitePath, "public", "llms.txt");

  if (!fs.existsSync(llmsPath)) {
    console.warn("llms.txt not found, skipping");
    return null;
  }

  const content = fs.readFileSync(llmsPath, "utf8");
  const headings = [...content.matchAll(/^##[ \t]+(?:zorath-env(?:[ \t]+\(zenv\))?|zenv)[ \t]*\r?$/gim)];
  if (headings.length !== 1) {
    throw new Error("llms.txt must contain exactly one zenv product section");
  }
  const start = headings[0].index + headings[0][0].length;
  const rest = content.slice(start);
  const nextSection = rest.search(/^#{1,2}[ \t]/m);
  const end = nextSection < 0 ? content.length : start + nextSection;
  const section = content.slice(start, end);
  const versionLine = /^- \*\*Version\*\*:[^\r\n]*/gm;
  if ([...section.matchAll(versionLine)].length !== 1) {
    throw new Error("The zenv section must contain exactly one version field");
  }
  return {
    path: llmsPath,
    content: content.slice(0, start) + section.replace(versionLine, `- **Version**: ${version}`) + content.slice(end),
  };
}

// Main sync function
function syncWebsite(version, websitePath) {
  if (!SEMVER.test(version) || /[\r\n]/.test(version)) {
    throw new Error("Version must be valid SemVer without a v prefix");
  }
  const dataFilePath = path.join(websitePath, "src", "data", "zenv.json");

  if (!fs.existsSync(dataFilePath)) {
    throw new Error(`Data file not found at ${dataFilePath}`);
  }

  // Read existing data
  const existingData = JSON.parse(fs.readFileSync(dataFilePath, "utf8"));
  if (!existingData || typeof existingData !== "object" || Array.isArray(existingData)) {
    throw new Error("zenv.json must contain a JSON object");
  }
  // Validate both destinations before changing either file.
  const llms = prepareLlmsTxt(version, websitePath);

  // Update version and lastUpdated
  const today = new Date().toISOString().split("T")[0];
  const updatedData = {
    ...existingData,
    version: version,
    lastUpdated: today,
  };

  // Write updated data
  fs.writeFileSync(dataFilePath, JSON.stringify(updatedData, null, 2) + "\n");

  console.log("Updated zenv.json:");
  console.log("  - version:", version);
  console.log("  - lastUpdated:", today);
  console.log("  - path:", dataFilePath);

  // Also update llms.txt
  if (llms) {
    fs.writeFileSync(llms.path, llms.content);
    console.log("Updated llms.txt:", llms.path);
  }

  return true;
}

// Entry point
function main() {
  const args = parseArgs();

  // Get version from args or Cargo.toml
  let version = args.version;
  if (!version) {
    version = getVersionFromCargo();
    if (!version) {
      console.error("Error: Could not determine version.");
      console.error("Provide --version argument or ensure Cargo.toml exists.");
      process.exit(1);
    }
    console.log("Using version from Cargo.toml:", version);
  }

  // Get website path from args or environment variable or default
  let websitePath = args.websitePath;
  if (!websitePath) {
    websitePath = process.env.ZORL_WEBSITE_PATH;
  }
  if (!websitePath) {
    // Default relative path from zenv repo
    websitePath = path.join(__dirname, "..", "..", "zorl");
  }

  // Resolve to absolute path
  websitePath = path.resolve(websitePath);
  console.log("Website path:", websitePath);

  if (!fs.existsSync(websitePath)) {
    console.error("Error: Website path does not exist:", websitePath);
    process.exit(1);
  }

  // Sync the website
  const success = syncWebsite(version, websitePath);

  if (success) {
    console.log("\nSync complete!");
    console.log("Next steps:");
    console.log("  1. cd", websitePath);
    console.log("  2. git add src/data/zenv.json public/llms.txt");
    console.log("  3. git commit -m 'Update zenv to v" + version + "'");
    console.log("  4. git push");
  }
}

if (require.main === module) {
  try { main(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}

module.exports = { parseArgs, syncWebsite };
