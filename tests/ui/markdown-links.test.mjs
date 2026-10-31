// A file an answer links to is named by the file alone; a web link is a link.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {markdown, filePath} from '../../ui/markdown.js';

test('a file:// link is read as a path, and a line if it names one', () => {
  deepEqual(filePath('file:///Users/r/My%20Dir/a.tex#L37'), {path: '/Users/r/My Dir/a.tex', line: 37});
  deepEqual(filePath('file:///Users/r/a.c#L3-L9'), {path: '/Users/r/a.c', line: 3});
  equal(filePath('https://example.com/a.tex'), null);
  equal(filePath('file://relative/a.tex'), null);
});

test('an answer names a linked file by its name, keeping the path for over it and a click', () => {
  const html = markdown('I read [Long_Review.tex](file:///Users/r/Github/Research/TinyML/Long_Review.tex) and [this](https://example.com).');
  ok(html.includes('<span class="file-link" data-file-link="/Users/r/Github/Research/TinyML/Long_Review.tex"'), html);
  ok(html.includes('>Long_Review.tex</span>') && !html.includes('](file:'), html);
  ok(html.includes('<a href="https://example.com" data-link'), html);
  // A label that is itself a path is shortened to its file too.
  ok(markdown('[/a/b/c.rs](file:///a/b/c.rs#L4)').includes('data-file-line="4" title="/a/b/c.rs · line 4">c.rs</span>'));
});
