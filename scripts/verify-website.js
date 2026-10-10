#!/usr/bin/env node
// Read-only verification. The website repository owns its write credentials.
const fs = require('node:fs');
const path = require('node:path');

async function verifyWebsite(version, fetchImpl = fetch) {
  version = version.replace(/^v/, '');
  if (!/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(version) || /[\r\n]/.test(version)) {
    throw new Error('Expected a stable version such as 0.3.13');
  }
  for (const resource of ['zenv', 'zenv/docs', 'llms.txt']) {
    const url = `https://zorl.cloud/${resource}`;
    const response = await fetchImpl(url, {
      headers: { 'User-Agent': 'zenv-website-verification', 'Cache-Control': 'no-cache' },
      redirect: 'error', signal: AbortSignal.timeout(30_000),
    });
    if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
    const body = await response.text();
    if (resource === 'llms.txt') {
      const sections = body.split(/^##[ \t]+zorath-env[ \t]+\(zenv\)[ \t]*\r?$/m);
      const section = sections[1]?.split(/^#{1,2}[ \t]/m)[0];
      const versions = [...(section || '').matchAll(/^- \*\*Version\*\*:[ \t]*(\S+)[ \t]*\r?$/gm)];
      if (sections.length !== 2 || versions.length !== 1 || versions[0][1] !== version) {
        throw new Error(`${url}: zenv version does not match ${version}`);
      }
    } else if (!new RegExp(`\\bv${version.replaceAll('.', '\\.')}\\b(?![.0-9-])`).test(body)) {
      throw new Error(`${url}: expected v${version} is missing`);
    }
    console.log(`Verified ${url}: v${version}`);
  }
}

if (require.main === module) {
  const version = process.env.EXPECTED_VERSION || fs.readFileSync(path.join(__dirname, '../Cargo.toml'), 'utf8').match(/^version = "([^"]+)"/m)?.[1];
  verifyWebsite(version || '').catch(error => { console.error(error.message); process.exitCode = 1; });
}

module.exports = { verifyWebsite };
