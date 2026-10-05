// The Literature tab's own logic: what the composer reads as a paper, and
// what the sources line says of each source and its key.
import {test} from 'node:test';
import {equal, ok} from 'node:assert/strict';
import {and, checkBadge, citationHtml, ownPaper, referenceHtml, sourcesLine} from '../../ui/literature.js';

test('/p and a DOI, an arXiv id or a link is a paper; anything else is not', () => {
  equal(ownPaper('/p 10.1364/JOSAA.18.000412'), '10.1364/JOSAA.18.000412');
  equal(ownPaper('  /paper https://arxiv.org/abs/1706.03762v5 '), 'https://arxiv.org/abs/1706.03762v5');
  equal(ownPaper('/p arXiv:2101.00001'), 'arXiv:2101.00001');
  equal(ownPaper('/press release'), null, 'a word that starts with p is not /p');
  equal(ownPaper('/p'), null);
  equal(ownPaper('Read 10.1364/JOSAA.18.000412'), null, 'a DOI in what is said to the team is said to the team');
});

const row = (source, name, chosen, key, resting = null) => ({source, name, chosen, key, resting});

test('the sources line says who is asked, whose key was found, and who is resting', () => {
  const rows = [row('arxiv', 'arXiv', true, 'notNeeded'), row('semanticScholar', 'Semantic Scholar', true, 'found'), row('openAlex', 'OpenAlex', true, 'missing')];
  const on = sourcesLine(rows, true, 0);
  ok(on.startsWith('Asks arXiv, Semantic Scholar (key found) and OpenAlex (no key: a hundred calls a day)'), on);
  ok(on.includes('never your files') && on.includes('checked with arXiv, Crossref'), on);
  equal(sourcesLine(rows, true, 0, false).includes('checked'), false, 'a team whose citations are not checked is not said to have them checked');
  ok(sourcesLine(rows, false, 0).startsWith('Off: nobody is asked anything.'));
  const resting = sourcesLine([row('openAlex', 'OpenAlex', true, 'missing', 10_000)], true, 5);
  ok(resting.includes('OpenAlex (out of calls until'), resting);
  equal(sourcesLine([row('openAlex', 'OpenAlex', true, 'missing', 10)], true, 50).includes('out of calls'), false, 'a rest that is over is not said');
  equal(sourcesLine([row('arxiv', 'arXiv', false, 'notNeeded')], true, 0), 'Asks no source: choose one above.');
  equal(sourcesLine([row('semanticScholar', 'Semantic Scholar', true, 'notLooked')], true, 0).startsWith('Asks Semantic Scholar,'), true, 'a key not looked for yet is not said to be missing');
  equal(and(['a']), 'a');
  equal(and(['a', 'b', 'c']), 'a, b and c');
});

test('a reference is the paper it names, a link to a DOI or an arXiv id, or what was written', () => {
  const papers = [{id: 'P003', title: 'Skylight <i>spectra</i>'}];
  const own = referenceHtml('P003', papers);
  ok(own.includes('data-lit-paper="P003"') && own.includes('Skylight &lt;i&gt;spectra&lt;/i&gt;'), own);
  ok(referenceHtml('[[literature/P003.pdf p. 4]]', papers).includes('data-lit-paper="P003"'), 'a page of a paper is the paper');
  ok(referenceHtml('doi:10.1364/JOSAA.18.000412', []).includes('href="https://doi.org/10.1364/JOSAA.18.000412"'));
  ok(referenceHtml('https://doi.org/10.1364/josaa.18.000412', []).includes('href="https://doi.org/10.1364/josaa.18.000412"'));
  ok(referenceHtml('arXiv:hep-th/9901001', []).includes('href="https://arxiv.org/abs/hep-th/9901001"'));
  const said = referenceHtml('Rayleigh (1871) <script>', []);
  ok(said.includes('&lt;script&gt;') && !said.includes('<script>'), said);
});

test('a cited name is marked with what checking found, and a paper the investigation has is the paper', () => {
  const check = (key, check, extra = {}) => ({text: key.replace(/^doi:/, ''), key, check, title: null, paper: null, ...extra});
  ok(checkBadge(check('doi:10.9999/made.up', 'unverified')).includes('data-check="unverified"'));
  ok(checkBadge(check('doi:10.9999/made.up', 'unverified')).includes('not found'));
  ok(checkBadge(check('arxiv:1706.03762', 'verified', {title: 'Attention'})).includes('title="arxiv:1706.03762 — Found: Attention"'));
  ok(checkBadge(check('doi:10.1/x', null)).includes('data-check="pending"'), 'not yet checked');
  ok(checkBadge(check('doi:10.1/x', 'verified', {paper: 'P003'})).includes('data-lit-paper="P003"'));
  const line = citationHtml('Smith (2001), 10.9999/made.up, from memory', [check('doi:10.9999/made.up', 'unverified'), check('doi:10.1/other', 'verified')]);
  ok(line.includes('data-check="unverified"') && !line.includes('data-check="verified"'), line);
  ok(citationHtml('As [[literature/P003.pdf p. 4]] shows', []).includes('data-cite="literature/P003.pdf p. 4"'), 'a cited page is a link');
  ok(referenceHtml('doi:10.9999/made.up', [], [check('doi:10.9999/made.up', 'unverified')]).includes('data-check="unverified"'));
});
