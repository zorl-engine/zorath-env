const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { parseArgs, syncWebsite } = require("./sync-website");

test("sync preserves website fields and replaces entire prerelease versions", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "zenv-sync-test-"));
  try {
    fs.mkdirSync(path.join(root, "src/data"), { recursive: true });
    fs.mkdirSync(path.join(root, "public"));
    const data = path.join(root, "src/data/zenv.json");
    const llms = path.join(root, "public/llms.txt");
    fs.writeFileSync(data, JSON.stringify({ version: "1.0.0-old", custom: "preserved" }));
    const beforeText = "# Site\r\n## Other tool\r\n- **Version**: 9.9.9\r\n## zorath-env (zenv)\r\n- **Version**: 1.0.0-old+metadata\r\nOther content\r\n## Last tool\r\n- **Version**: 8.8.8\r\n";
    fs.writeFileSync(llms, beforeText);
    syncWebsite("1.2.3-beta-1+001", root);
    assert.equal(JSON.parse(fs.readFileSync(data)).custom, "preserved");
    assert.equal(JSON.parse(fs.readFileSync(data)).version, "1.2.3-beta-1+001");
    assert.equal(fs.readFileSync(llms, "utf8"), beforeText.replace("1.0.0-old+metadata", "1.2.3-beta-1+001"));
    const before = fs.readFileSync(data, "utf8");
    for (const version of ["01.2.3", "1.2.3-01", "1.2.3\ncommand", "$(command)"]) {
      assert.throws(() => syncWebsite(version, root));
      assert.equal(fs.readFileSync(data, "utf8"), before);
    }
  } finally {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(root).startsWith("zenv-sync-test-"));
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("invalid website metadata fails before changing either file", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "zenv-sync-test-"));
  try {
    fs.mkdirSync(path.join(root, "src/data"), { recursive: true });
    fs.mkdirSync(path.join(root, "public"));
    const data = path.join(root, "src/data/zenv.json");
    const llms = path.join(root, "public/llms.txt");
    const validData = '{"version":"1.0.0"}';
    const validText = "## zorath-env (zenv)\n- **Version**: 1.0.0\n";
    for (const [jsonText, llmsText] of [
      ["null", validText], ["[]", validText], ['"text"', validText],
      [validData, "## Other tool\n- **Version**: 9.9.9\n"],
      [validData, "## zenv\nNo version\n"],
      [validData, validText + "- **Version**: 2.0.0\n"],
      [validData, validText + validText],
    ]) {
      fs.writeFileSync(data, jsonText);
      fs.writeFileSync(llms, llmsText);
      assert.throws(() => syncWebsite("1.2.3", root));
      assert.equal(fs.readFileSync(data, "utf8"), jsonText);
      assert.equal(fs.readFileSync(llms, "utf8"), llmsText);
    }
  } finally {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(root).startsWith("zenv-sync-test-"));
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("unknown and incomplete options fail", () => {
  assert.throws(() => parseArgs(["--versoin", "1.2.3"]));
  assert.throws(() => parseArgs(["--website-path"]));
  assert.deepEqual(parseArgs(["--version", "1.2.3", "--website-path", "folder with spaces"]), {
    version: "1.2.3", websitePath: "folder with spaces",
  });
});
