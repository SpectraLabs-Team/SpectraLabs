// M11 Task 5b: Accessibility tests for attribution card. Run: node scripts/test-attribution-a11y.mjs
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');

const cardSource = fs.readFileSync(
  path.join(root, 'components/studio/backtests/AttributionCard.tsx'),
  'utf8'
);

describe('attribution card accessibility (M11 Task 5b)', () => {
  it('glossary dialog declares role="dialog", aria-modal="true", and aria-labelledby', () => {
    assert.match(cardSource, /role="dialog"/, 'dialog must declare role="dialog"');
    assert.match(cardSource, /aria-modal="true"/, 'dialog must declare aria-modal="true"');
    assert.match(
      cardSource,
      /aria-labelledby="attribution-glossary-title"/,
      'dialog must be labelled by attribution-glossary-title'
    );
    assert.match(
      cardSource,
      /id="attribution-glossary-title"/,
      'header title element must have matching id'
    );
  });

  it('glossary dialog keeps Tab focus cycling forward inside the modal', () => {
    assert.match(cardSource, /dialogRef/, 'must use dialogRef to scope focusable elements');
    assert.match(cardSource, /e\.key !== 'Tab'/, 'must check for Tab key');
    assert.match(cardSource, /querySelectorAll<HTMLElement>/, 'must query focusable elements');
    assert.match(
      cardSource,
      /!e\.shiftKey\s*&&\s*\(active === last\s*\|\|\s*!dialogRef\.current\.contains\(active\)\)[\s\S]*?first\.focus\(\)/,
      'must wrap forward Tab navigation from last focusable to first'
    );
  });

  it('glossary dialog handles Shift+Tab reverse cycling inside the focus trap', () => {
    assert.match(
      cardSource,
      /e\.shiftKey\s*&&\s*\(active === first\s*\|\|\s*!dialogRef\.current\.contains\(active\)\)[\s\S]*?last\.focus\(\)/,
      'must wrap reverse Shift+Tab navigation from first focusable to last'
    );
  });

  it('glossary dialog listens for Escape globally to allow dismissing from anywhere', () => {
    assert.match(
      cardSource,
      /window\.addEventListener\(['"]keydown['"],\s*onKey\)/,
      'must register global window keydown listener for Escape'
    );
    assert.match(
      cardSource,
      /window\.removeEventListener\(['"]keydown['"],\s*onKey\)/,
      'must clean up global keydown listener'
    );
    assert.match(
      cardSource,
      /if\s*\(e\.key === 'Escape'\)[\s\S]*?onClose\(\)/,
      'Escape key must trigger onClose'
    );
  });

  it('glossary dialog focuses the search input on open (focus in)', () => {
    assert.match(cardSource, /searchRef\s*=\s*useRef<HTMLInputElement>/);
    assert.match(
      cardSource,
      /searchRef\.current\?\.focus\(\)/,
      'must auto-focus searchRef on modal open'
    );
  });

  it('glossary dialog restores focus back to the opener element on close (focus out)', () => {
    assert.match(
      cardSource,
      /const opener\s*=\s*document\.activeElement/,
      'must capture document.activeElement on modal open'
    );
    assert.match(
      cardSource,
      /opener\.focus\(\)/,
      'must return focus to opener element when modal closes'
    );
  });

  it('category filter tabs declare toggle state with aria-pressed', () => {
    assert.match(
      cardSource,
      /aria-pressed=\{category === cat\}/,
      'category buttons must declare aria-pressed reflecting active category'
    );
    assert.match(
      cardSource,
      /role="group"/,
      'category tab container should define role="group" with accessible label'
    );
  });

  it('search box has a name attribute and accessible label', () => {
    assert.match(
      cardSource,
      /<input[^>]*type="search"[^>]*name=["']glossary-search["']/,
      'search box must have a name attribute'
    );
    assert.match(
      cardSource,
      /<input[^>]*aria-label="Search glossary terms"/,
      'search box must have an accessible aria-label'
    );
  });

  it('both waterfall and instruments tables have captions for screen readers', () => {
    assert.match(
      cardSource,
      /<caption className="sr-only">[\s\S]*?waterfall[\s\S]*?<\/caption>/i,
      'waterfall table must have a screen-reader caption'
    );
    assert.match(
      cardSource,
      /<caption className="sr-only">[\s\S]*?instrument[\s\S]*?<\/caption>/i,
      'instruments table must have a screen-reader caption'
    );
  });

  it('loading card specifies role="status" and help buttons have stronger focus rings', () => {
    assert.match(
      cardSource,
      /<GlassCard[^>]*role="status"/,
      'AttributionPanel loading state must declare role="status"'
    );
    assert.match(
      cardSource,
      /focus-visible:ring-2 focus-visible:ring-accent/,
      'help buttons must have stronger focus-visible:ring-2'
    );
  });
});
