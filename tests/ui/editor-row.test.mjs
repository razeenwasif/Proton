import {test} from 'node:test';
import assert from 'node:assert/strict';
import {lineHtml} from '../../ui/editor-row.js';

const context = (selection = null, cursor = {line: 0, column: 0}, vim = 'VISUAL') =>
  ({pane: {selection, cursor}, vim, diagnostics: new Map()});
const row = (text, state = context(), options = {}) => lineHtml(options.line ?? 0, text,
  options.caret, options.highlights, options.hits, options.git, options.stop, state);

test('reverse character selection includes both endpoints on one line', () => {
  const state = context({line: 0, column: 4}, {line: 0, column: 1});
  assert.match(row('abcdef', state), /a<span class="sel">bcde<\/span>f/);
});

test('Unicode columns place the caret and selection on whole characters', () => {
  const state = context({line: 0, column: 1}, {line: 0, column: 2});
  assert.match(row('a😀βz', state, {caret: 2}), /a<span class="sel">😀<\/span><span class="caret">β<\/span>z/);
});

test('rendering another pane never borrows a previous pane selection', () => {
  const selected = context({line: 0, column: 0}, {line: 0, column: 2});
  assert.match(row('abc', selected), /class="sel"/);
  assert.doesNotMatch(row('abc'), /class="sel"/);
  assert.match(row('abc', selected), /class="sel"/);
});

test('multiline, block, and empty line selections keep their shapes', () => {
  const state = context({line: 2, column: 1}, {line: 0, column: 3});
  assert.match(row('abcde', state), /abc<span class="sel">de<\/span>/);
  assert.match(row('abcde', state, {line: 2}), /<span class="sel">ab<\/span>cde/);
  assert.match(row('', {...state, vim: 'V-LINE'}, {line: 1}), /class="ln selected"/);
  assert.match(row('abcde', {...state, vim: 'V-BLOCK'}, {line: 1}), /a<span class="sel">bcd<\/span>e/);
});

test('overlapping diagnostics preserve the strongest severity with syntax and search', () => {
  const state = context();
  state.diagnostics.set(0, [
    {line: 0, column: 1, endLine: 0, endColumn: 2, severity: 'error'},
    {line: 0, column: 0, endLine: 0, endColumn: 3, severity: 'warning'},
  ]);
  const html = row('abc', state, {highlights: [{start: 0, end: 3, scope: 'keyword'}], hits: [{start: 1, end: 2}]});
  assert.match(html, /class="k sq d-warning">a/);
  assert.match(html, /class="k sq d-error hit">b/);
  assert.match(html, /class="k sq d-warning">c/);
});

test('source HTML remains text and long highlighted runs stay merged', () => {
  assert.match(row('<script>"&'), /&lt;script&gt;&quot;&amp;/);
  const html = row('x'.repeat(10000), context(), {highlights: [{start: 0, end: 10000, scope: 'string'}]});
  assert.equal((html.match(/class="str"/g) ?? []).length, 1);
});
