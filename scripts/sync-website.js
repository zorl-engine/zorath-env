#!/usr/bin/env node
/**
 * sync-website.js
 *
 * Syncs zenv data to the zorl.cloud website repository.
 * Run this script after version bumps to keep the website in sync.
 *
 * Usage:
 *   node scripts/sync-website.js --version 0.2.3
 *   node scripts/sync-website.js --version 0.2.3 --website-path ../zorl.cloud
 */

const fs = require("fs");
const path = require("path");

// Parse command line arguments
function parseArgs() {
  const args = process.argv.slice(2);
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

// Read README.md to extract features
function extractFeaturesFromReadme() {
  const readmePath = path.join(__dirname, "..", "README.md");
  if (!fs.existsSync(readmePath)) {
    console.warn("README.md not found, using default features");
    return null;
  }

  // Features are extracted from the existing data file
  // README is used to validate but not as the source of truth
  return null;
}

// Update llms.txt with new version
function updateLlmsTxt(version, websitePath) {
  const llmsPath = path.join(websitePath, "public", "llms.txt");

  if (!fs.existsSync(llmsPath)) {
    console.warn("llms.txt not found, skipping");
    return false;
  }

  let content = fs.readFileSync(llmsPath, "utf8");

  // Update version line
  content = content.replace(
    /- \*\*Version\*\*: [\d.]+/,
    `- **Version**: ${version}`
  );

  fs.writeFileSync(llmsPath, content);
  console.log("Updated llms.txt:");
  console.log("  - version:", version);
  console.log("  - path:", llmsPath);

  return true;
}

// Main sync function
function syncWebsite(version, websitePath) {
  const dataFilePath = path.join(websitePath, "src", "data", "zenv.json");

  if (!fs.existsSync(dataFilePath)) {
    console.error("Error: Data file not found at", dataFilePath);
    console.error("Make sure the website repository path is correct.");
    process.exit(1);
  }

  // Read existing data
  const existingData = JSON.parse(fs.readFileSync(dataFilePath, "utf8"));

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
  updateLlmsTxt(version, websitePath);

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
    websitePath = path.join(__dirname, "..", "..", "zorl.cloud");
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
    console.log("  2. git add src/data/zenv.json");
    console.log("  3. git commit -m 'Update zenv to v" + version + "'");
    console.log("  4. git push");
  }
}

main();
