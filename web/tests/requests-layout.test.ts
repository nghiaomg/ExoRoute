/**
 * Layout invariants for the Requests page sections.
 *
 * `.content-wrap` in `styles/base.css` is the dashboard's scrolling column and is
 * a flex container (`display: flex; flex-direction: column`). Every page section
 * is therefore a flex item with the default `flex-shrink: 1`.
 *
 * A flex item that clips its own overflow (`overflow: hidden`, or `auto` on the
 * cross axis) has an automatic minimum size of zero. Such an item can absorb all
 * of the column's negative free space and collapse to almost nothing, which is
 * how the `.statistics-details` panel once rendered as a thin ink strip beneath
 * the pager. The panel needs `overflow: hidden` to clip its grid cells inside the
 * rounded frame, so it must opt out of shrinking explicitly.
 *
 * jsdom returns empty computed styles for component `<style>` blocks, so a
 * rendered-DOM assertion cannot catch this regression. These tests read the
 * component source instead and check the declarations that decide the outcome.
 */
import { strict as assert } from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const detailsSource = readFileSync(
  new URL('../src/features/requests/RequestStatisticsDetails.svelte', import.meta.url),
  'utf8',
);
const filterPanelSource = readFileSync(
  new URL('../src/features/requests/RequestFilterPanel.svelte', import.meta.url),
  'utf8',
);
const tableSource = readFileSync(
  new URL('../src/features/requests/RequestsTable.svelte', import.meta.url),
  'utf8',
);
const baseCss = readFileSync(new URL('../src/styles/base.css', import.meta.url), 'utf8');

/** Returns the declarations of the first rule whose selector list contains `selector`. */
function declarationsFor(css: string, selector: string): Record<string, string> {
  const selectorIndex = css.indexOf(selector);
  assert.notEqual(selectorIndex, -1, `missing CSS rule: ${selector}`);
  const openBrace = css.indexOf('{', selectorIndex);
  const closeBrace = css.indexOf('}', openBrace);
  assert.ok(openBrace > selectorIndex && closeBrace > openBrace, `invalid CSS rule: ${selector}`);
  return Object.fromEntries(
    css
      .slice(openBrace + 1, closeBrace)
      .split(';')
      .flatMap((declaration) => {
        const separator = declaration.indexOf(':');
        if (separator < 0) return [];
        const name = declaration.slice(0, separator).split(/\r?\n/).at(-1)?.trim();
        const value = declaration.slice(separator + 1).trim();
        return name ? [[name, value]] : [];
      }),
  );
}

test('the dashboard column is the scrolling flex column these invariants describe', () => {
  // If .content-wrap stops being a flex column, the shrink rules below no longer
  // describe the real layout and this test should be revisited rather than
  // silently passing against a stale premise.
  const declarations = declarationsFor(baseCss, '.content-wrap {');
  assert.equal(declarations.display, 'flex');
  assert.equal(declarations['flex-direction'], 'column');
});

test('the statistics panel does not shrink inside the scrolling column', () => {
  const declarations = declarationsFor(detailsSource, '.statistics-details {');

  // The clipping is required: it crops the 1px grid gaps and cell backgrounds
  // to the rounded frame.
  assert.equal(declarations.overflow, 'hidden');

  // Without this the panel collapses to its 2px borders whenever the column's
  // content overflows — the reported thin-strip bug.
  assert.equal(declarations['flex-shrink'], '0');

  // The panel is a six-column grid; shrinking must not be reintroduced through
  // a flex shorthand that resets flex-shrink.
  assert.ok(!('flex' in declarations), 'the panel should not use a flex shorthand');
});

test('the statistics panel keeps its grid tracks and cell padding', () => {
  const panel = declarationsFor(detailsSource, '.statistics-details {');
  assert.equal(panel['grid-template-columns'], 'repeat(6, minmax(0, 1fr))');

  // Each cell paints its own background, so the frame reads as six tiles.
  const cell = declarationsFor(detailsSource, '.statistics-details > div {');
  assert.equal(cell.display, 'grid');
  assert.equal(cell['min-width'], '0');
  assert.ok(cell.padding, 'cells keep padding so the panel has real height');

  // Both responsive track counts still exist.
  assert.match(detailsSource, /\.statistics-details \{ grid-template-columns: repeat\(3, minmax\(0, 1fr\)\); \}/);
  assert.match(detailsSource, /\.statistics-details \{ grid-template-columns: 1fr 1fr; \}/);
});

test('the statistics panel follows the theme system outside classic light', () => {
  // In dark themes `--ink` is near-white, so the light rule's 2px ink frame and
  // hard offset shadow would render as a glaring white strip around the glass
  // panel. The api-key statistics tokens panel (dark overrides in
  // styles/theme-shell.css) is the reference treatment these mirror.
  const darkFrame = declarationsFor(
    detailsSource,
    ":global(:root[data-theme='dark']) .statistics-details {",
  );
  assert.equal(darkFrame.background, 'rgba(255, 255, 255, 0.07)');
  assert.equal(darkFrame['border-color'], 'rgba(255, 255, 255, 0.07)');

  // Cells keep the same glass fill the dark tokens panel uses.
  const darkCell = declarationsFor(
    detailsSource,
    ":global(:root[data-theme='dark']) .statistics-details > div {",
  );
  assert.equal(darkCell.background, 'color-mix(in srgb, var(--surface-glass-base) 72%, transparent)');

  // Elevation comes from the per-theme --nb-shadow-md token (classic light
  // resolves it to the identical ink offset), not a hardcoded light shadow.
  const panel = declarationsFor(detailsSource, '.statistics-details {');
  assert.equal(panel['box-shadow'], 'var(--nb-shadow-md, 3px 3px 0 var(--ink))');
});

test('the filter panel stays a normal flow sibling of the table card', () => {
  // A sticky filter panel pinned itself to the scrolling column while the table
  // card kept scrolling, so the two framed cards rendered stacked on top of
  // each other on laptop-height displays. The panel must stay an ordinary flex
  // sibling of the table card so nothing can overlap it.
  const panel = declarationsFor(filterPanelSource, '.request-filter-panel {');
  assert.ok(!('position' in panel), 'the filter panel must stay in normal flow');
  assert.ok(!('z-index' in panel), 'an in-flow panel needs no stacking order');
});

test('the requests table renders at full height in the page column', () => {
  // A fixed-height internal scroll viewport wrapped the rows in a second
  // scroll container beside the page column and the table's own horizontal
  // one, which read as stacked containers with cut-off rows. The card now
  // grows with its rows — the same full-height pattern as the overview request
  // table and the mobile card feed — so the page column is the only vertical
  // scroller.
  const card = declarationsFor(tableSource, '.request-table-card {');
  assert.ok(!('max-height' in card), 'the card must not cap its height');
  assert.ok(!('min-height' in card), 'the card must not force a height floor');
  assert.ok(!('overflow-y' in card), 'the card must not scroll vertically');

  // overflow-x keeps narrow viewports able to pan the columns, which also
  // makes the card a scroll container with a zero automatic minimum size —
  // the same flex hazard the statistics panel hit — so it must opt out.
  assert.equal(card['overflow-x'], 'auto');
  assert.equal(card['flex-shrink'], '0');

  // Sticky against the page column rendered unreliably, and the card no
  // longer scrolls internally, so the header simply scrolls with the page.
  const header = declarationsFor(tableSource, '.request-table-card thead th {');
  assert.ok(!('position' in header), 'the header must not be sticky');
});
