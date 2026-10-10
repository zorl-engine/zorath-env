const { test } = require('node:test');
const assert = require('node:assert/strict');
const { verifyWebsite } = require('./verify-website');

const page = '<p>zorath-env v0.3.13</p>';
const llms = '## Other\n- **Version**: 9.9.9\n## zorath-env (zenv)\n- **Version**: 0.3.13\n## Last\n';
const responses = (text = llms, html = page) => async url => ({ ok: true, text: async () => url.endsWith('llms.txt') ? text : html });

test('verifies all three public pages and accepts a tag prefix', async () => {
  await verifyWebsite('v0.3.13', responses());
});

test('rejects an unrelated product version, duplicates, missing pages, and stale pages', async () => {
  for (const text of [llms.replace('0.3.13', '0.3.12'), llms.replace('## zorath-env (zenv)', '## Different'), llms + llms, llms.replace('## Last', '- **Version**: 0.3.13\n## Last')]) {
    await assert.rejects(verifyWebsite('0.3.13', responses(text)), /version does not match/);
  }
  for (const html of ['v0.3.12', 'v0.3.130', 'v0.3.13-beta.1']) {
    await assert.rejects(verifyWebsite('0.3.13', responses(llms, html)), /missing/);
  }
  await assert.rejects(verifyWebsite('0.3.13', async () => ({ ok: false, status: 503 })), /HTTP 503/);
});

test('rejects invalid versions before network access', async () => {
  for (const version of ['0.3.13\n', '00.3.13', '0.3.13-beta.1', '$(command)']) {
    await assert.rejects(verifyWebsite(version, () => assert.fail('Unexpected network request')), /stable version/);
  }
});
