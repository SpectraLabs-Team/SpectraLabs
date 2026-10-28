import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendDir = path.resolve(__dirname, '..');

test('Next.js 16 configuration and image optimization', async (t) => {
  await t.test('next.config.js removes deprecated eslint config key', () => {
    const configPath = path.join(frontendDir, 'next.config.js');
    const content = fs.readFileSync(configPath, 'utf8');

    assert.equal(
      content.includes('eslint:'),
      false,
      'next.config.js should not contain deprecated eslint configuration block'
    );
    assert.ok(content.includes('poweredByHeader: false'), 'Retains standard security config');
    assert.ok(content.includes('serverExternalPackages'), 'Retains server external packages');
  });

  await t.test('primitives Logo component uses optimized next/image Image component', () => {
    const primitivesPath = path.join(frontendDir, 'components/ui/primitives.tsx');
    const content = fs.readFileSync(primitivesPath, 'utf8');

    assert.ok(
      content.includes("import Image from 'next/image';"),
      'Imports Image from next/image'
    );
    assert.ok(
      content.includes('<Image'),
      'Renders <Image component'
    );
    assert.equal(
      content.includes('<img'),
      false,
      'No raw <img> tags should exist in primitives.tsx'
    );
    assert.ok(
      content.includes('priority'),
      'Logo image has priority attribute for fast LCP above the fold'
    );
  });
});
