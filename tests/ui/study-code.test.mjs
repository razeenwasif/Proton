// What the coding practice editor's keys do to code: the Codex editor's
// typing rules, as a textarea has them. `|` marks the caret, `[` and `]` a
// selection.
import {test} from 'node:test';
import {equal} from 'node:assert/strict';
import {indentUnit, keyChange, toggleComment} from '../../ui/study-code.js';

/** Code with its caret or selection marked, as the editor has it. @param {string} marked */
function at(marked) {
  const caret = marked.indexOf('|');
  if (caret >= 0) return {value: marked.replace('|', ''), start: caret, end: caret};
  const start = marked.indexOf('[');
  const end = marked.indexOf(']') - 1;
  return {value: marked.replace('[', '').replace(']', ''), start, end};
}

/** What a key makes of marked code, marked again; unchanged if the key is
 *  left to the browser. @param {string} marked @param {string} key */
function press(marked, key, {shift = false, unit = '    ', lang = 'python'} = {}) {
  const now = at(marked);
  const c = keyChange(now, key, {shift, unit, lang});
  if (!c) return marked;
  const value = now.value.slice(0, c.from) + c.text + now.value.slice(c.to);
  return c.start === c.end ? `${value.slice(0, c.start)}|${value.slice(c.start)}`
    : `${value.slice(0, c.start)}[${value.slice(c.start, c.end)}]${value.slice(c.end)}`;
}

test('code is indented by its own step: the commonest rise, or tabs, or the language\'s', () => {
  equal(indentUnit('def f(x):\n  if x:\n    return 1\n  return 0\n', '    '), '  ');
  equal(indentUnit('int f(void) {\n\treturn 0;\n}\n', '    '), '\t');
  equal(indentUnit('x = 1\n', '    '), '    ');
});

test('Tab indents to the next stop, and a selection of lines a step; Shift-Tab takes one off', () => {
  equal(press('x|', 'Tab'), 'x   |');
  equal(press('|x', 'Tab', {unit: '\t', lang: 'c'}), '\t|x');
  equal(press('[a\nb]', 'Tab'), '[    a\n    b]');
  equal(press('    a|', 'Tab', {shift: true}), 'a|');
  equal(press('[    a\n  b]', 'Tab', {shift: true}), '[a\nb]');
});

test('Enter keeps the indent, opens the block a bracket or Python\'s colon begins, and puts a closer on its own line', () => {
  equal(press('    x = 1|', 'Enter'), '    x = 1\n    |');
  equal(press('def f():|', 'Enter'), 'def f():\n    |');
  equal(press('if (x) {|}', 'Enter', {lang: 'c'}), 'if (x) {\n    |\n}');
  equal(press('  f(|)', 'Enter', {unit: '  ', lang: 'javascript'}), '  f(\n    |\n  )');
  equal(press('x: int = 1|', 'Enter', {lang: 'rust'}), 'x: int = 1\n|', 'only Python opens a block with a colon');
});

test('brackets and quotes close themselves where a word does not follow, and are stepped over', () => {
  equal(press('f|', '('), 'f(|)');
  equal(press('|x', '('), '|x', 'not before a word');
  equal(press('f(|)', ')'), 'f()|');
  equal(press('[x]', '['), '[[x]]', 'a selection is put between them');
  equal(press('s = |', '"'), 's = "|"');
  equal(press('s = "|"', '"'), 's = ""|');
  equal(press("don|", "'"), "don|", 'not after a letter');
  equal(press('s = ""|', '"'), 's = """|"""', 'the third of Python\'s quotes');
  equal(press("x: &|", "'", {lang: 'rust'}), "x: &|", 'a lifetime');
});

test('Backspace takes an empty pair whole, and the indent a step at a time', () => {
  equal(press('f(|)', 'Backspace'), 'f|');
  equal(press('"|"', 'Backspace'), '|');
  equal(press('      |', 'Backspace'), '    |');
  equal(press('    x|', 'Backspace'), '    x|', 'a letter is the browser\'s');
});

test('a closer typed on a line of indent alone goes back a step', () => {
  equal(press('if (x) {\n    y();\n    |', '}', {lang: 'c'}), 'if (x) {\n    y();\n}|');
  equal(press('\t\t|', '}', {unit: '\t', lang: 'c'}), '\t}|');
});

test('⌘/ comments the lines selected, and takes the comments off when they all have one', () => {
  const comment = (/** @type {string} */ marked, /** @type {string} */ token) => {
    const now = at(marked);
    const c = toggleComment(now, token);
    if (!c) return marked;
    const value = now.value.slice(0, c.from) + c.text + now.value.slice(c.to);
    return c.start === c.end ? `${value.slice(0, c.start)}|${value.slice(c.start)}` : `${value.slice(0, c.start)}[${value.slice(c.start, c.end)}]${value.slice(c.end)}`;
  };
  equal(comment('[  a\n    b]', '#'), '[  # a\n  #   b]');
  equal(comment('[  # a\n  #   b]', '#'), '[  a\n    b]');
  equal(comment('x = 1|', '//'), '// x = 1|');
});
