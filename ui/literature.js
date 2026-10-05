/* The literature: the papers an investigation keeps, the sources it asks,
 * and the scientist's searches — drawn for Research's Literature tab.
 *
 * Everything a source wrote — a title, an abstract, a venue — is someone
 * else's text: it is escaped, or drawn through markdown(), which escapes
 * it, and its links open in the browser like any other (a[data-link]). Of a
 * key, all that is ever drawn is whether it was found. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";

/** @typedef {import("./bindings/LiteratureSourceRow").LiteratureSourceRow} SourceRow */
/** @typedef {import("./bindings/LiteratureSource").LiteratureSource} Source */
/** @typedef {import("./bindings/PaperRow").PaperRow} PaperRow */
/** @typedef {import("./bindings/PaperDetail").PaperDetail} PaperDetail */
/** @typedef {import("./bindings/PaperResult").PaperResult} PaperResult */
/** @typedef {{slug: string, enabled: boolean, live: boolean, sources: SourceRow[], papers: PaperRow[], fetching: string[], adding: string[]}} LiteratureEvent */
/** @typedef {{slug: string, detail: PaperDetail | null}} PaperEvent */
/** @typedef {{slug: string, query: string, searching: boolean, results: PaperResult[], failures: string[]}} ResultsEvent */

/** What a source without its key is like, and the variable that gives it one. */
const KEYLESS = /** @type {Record<string, {note: string, variable: string, more: string}>} */ ({
  semanticScholar: { note: "no key: a pool shared by everybody, often busy", variable: "SEMANTIC_SCHOLAR_API_KEY", more: "gives Proton its own rate" },
  openAlex: { note: "no key: a hundred calls a day", variable: "OPENALEX_API_KEY", more: "gives more calls" },
});

/** What each source is for. */
const ABOUT = /** @type {Record<string, string>} */ ({
  arxiv: "Physics, mathematics and computer science preprints, every one with its PDF.",
  semanticScholar: "Every field, with how often each paper is cited.",
  openAlex: "Everything, with where an open copy of each paper is.",
});

/** `/p 10.1364/JOSAA.18.000412` — a paper of the scientist's choosing, by a
 *  DOI, an arXiv id, or a link to either. What follows `/p`, or null.
 *  @param {string} text @returns {string | null} */
export function ownPaper(text) {
  const m = /^\/p(?:aper)?\s+(\S[\s\S]*)$/.exec(text.trim());
  return m ? m[1].trim() : null;
}

/** "a", "a and b", "a, b and c". @param {string[]} items */
export function and(items) {
  return items.length < 2 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** A time of day, from seconds since the epoch. @param {number} s */
function clock(s) {
  return new Date(s * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** What a source has to say for itself in the line: its key, or that it is
 *  out of calls. @param {SourceRow} row @param {number} now */
function note(row, now) {
  if (row.resting && row.resting > now) return `out of calls until ${clock(row.resting)}`;
  if (row.key === "found") return "key found";
  if (row.key === "missing") return KEYLESS[row.source]?.note ?? "no key";
  return "";
}

/** Which sources are asked, as a sentence: whose keys were found, which have
 *  none, who is resting — and what they are told.
 *  @param {SourceRow[]} rows @param {boolean} enabled @param {number} [now] */
export function sourcesLine(rows, enabled, now = Date.now() / 1000) {
  const chosen = rows.filter((r) => r.chosen);
  if (!chosen.length) return "Asks no source: choose one above.";
  const said = and(chosen.map((r) => {
    const n = note(r, now);
    return n ? `${r.name} (${n})` : r.name;
  }));
  return enabled
    ? `Asks ${said}, with the team's queries each round and yours when you search. They see the queries, never your files.`
    : `Off: nobody is asked anything. On, it asks ${said}, with the team's queries each round and yours when you search.`;
}

/** A source's chip: on when it is asked. @param {SourceRow} row @param {boolean} disabled @param {number} now */
function chip(row, disabled, now) {
  const keyless = KEYLESS[row.source];
  const title = [ABOUT[row.source] ?? row.name,
    row.key === "found" ? "Its key was found in your environment."
      : row.key === "missing" && keyless ? `No key was found: a free one, set as ${keyless.variable} in your shell's profile, ${keyless.more}.` : "",
    row.resting && row.resting > now ? `It said it was out of calls, and is left alone until ${clock(row.resting)}.` : "",
  ].filter(Boolean).join(" ");
  return `<button class="lit-source${row.chosen ? " on" : ""}" data-lit-source="${esc(row.source)}" aria-pressed="${row.chosen}"` +
    ` title="${esc(title)}"${disabled ? " disabled" : ""}>${row.chosen ? icon("check") : ""}${esc(row.name)}</button>`;
}

/** Authors, year and venue, as a reference says them.
 *  @param {{authors: string, year: number | null, venue: string | null}} p */
function byline(p) {
  return [p.authors && p.year ? `${p.authors} (${p.year})` : p.authors || (p.year ? String(p.year) : ""), p.venue ?? ""].filter(Boolean).join(" · ");
}

/** A paper's PDF, as a badge: kept, coming, or not to be had.
 *  @param {PaperRow} p @param {boolean} fetching */
function pdfBadge(p, fetching) {
  if (p.hasPdf) return `<span class="lit-badge" data-pdf="kept">PDF</span>`;
  if (fetching) return `<span class="lit-badge" data-pdf="fetching">Downloading…</span>`;
  return `<span class="lit-badge" data-pdf="none">${p.access === "closed" ? "Paywalled" : "Abstract only"}</span>`;
}

/** The literature tab's top: the switch, the sources and what they are told.
 *  @param {LiteratureEvent} lit @param {number} now */
function headHtml(lit, now) {
  return `<section class="lit-head">
      <label class="lit-switch"><input type="checkbox" data-lit-switch${lit.enabled ? " checked" : ""}>
        <span><b>Search the literature each round</b><small>The papers it finds are added to <code>literature/</code>, and given to every seat.</small></span></label>
      <div class="lit-sources" role="group" aria-label="Sources">${lit.sources.map((r) => chip(r, false, now)).join("")}</div>
      <p class="lit-line">${esc(sourcesLine(lit.sources, lit.enabled, now))}</p>
      ${lit.live ? "" : `<p class="lit-line rs-faint">This Proton asks nobody: a preview, or a test.</p>`}
    </section>`;
}

/** The search box, keeping what was typed. @param {LiteratureEvent} lit @param {string} query @param {boolean} searching */
function searchHtml(lit, query, searching) {
  const off = !lit.enabled || !lit.sources.some((r) => r.chosen);
  const asks = and(lit.sources.filter((r) => r.chosen).map((r) => r.name));
  return `<form class="lit-search" data-lit-search novalidate>
      <span class="lit-search-icon">${icon("search")}</span>
      <input name="query" data-lit-query value="${esc(query)}" autocomplete="off" spellcheck="false"
        placeholder="${off ? "Turn the literature on, and choose a source, to search" : `Search ${esc(asks)}`}"${off ? " disabled" : ""} aria-label="Search the literature">
      <button type="submit" class="cc-primary"${off || searching ? " disabled" : ""}>${searching ? "Searching…" : "Search"}</button>
    </form>`;
}

/** One result: what it is, and Add — or the paper it is already.
 *  @param {PaperResult} r @param {string[]} adding @param {boolean} enabled */
function resultHtml(r, adding, enabled) {
  const meta = [byline(r), r.citations != null ? `cited ${r.citations.toLocaleString()} times` : "", r.sources.map((s) => SOURCE_NAMES[s] ?? s).join(", ")].filter(Boolean).join(" · ");
  const pdf = r.pdf ? `<span class="lit-badge" data-pdf="open">Open PDF</span>`
    : `<span class="lit-badge" data-pdf="none">${r.access === "closed" ? "Paywalled" : "Abstract only"}</span>`;
  const action = r.have
    ? `<button class="rs-link" data-lit-paper="${esc(r.have)}" title="Already in the literature">${esc(r.have)}</button>`
    : adding.includes(r.key) ? `<span class="rs-faint">Adding…</span>`
    : `<button class="cc-quiet" data-lit-add="${esc(r.key)}"${enabled ? "" : ' disabled title="Turn the literature on to add papers"'}>${icon("plus")}Add</button>`;
  return `<article class="lit-result${r.have ? " have" : ""}">
      <div class="lit-result-text">
        <div class="lit-title">${esc(r.title)}</div>
        <div class="lit-meta">${esc(meta)}</div>
        ${r.abstract.trim() ? `<div class="lit-abstract">${esc(r.abstract)}</div>` : ""}
      </div>
      <div class="lit-result-side">${pdf}${action}</div>
    </article>`;
}

const SOURCE_NAMES = /** @type {Record<string, string>} */ ({ arxiv: "arXiv", semanticScholar: "Semantic Scholar", openAlex: "OpenAlex" });

/** The last search's results. @param {ResultsEvent | null} results @param {string[]} adding @param {boolean} enabled */
function resultsHtml(results, adding, enabled) {
  if (!results) return "";
  const failures = results.failures.map((f) => `<div class="cc-notice warn">${esc(f)}</div>`).join("");
  const list = results.results.map((r) => resultHtml(r, adding, enabled)).join("");
  const none = !results.searching && !results.results.length
    ? `<div class="cc-none rs-none">Nothing was found for “${esc(results.query)}”. Fewer, plainer words find more; a quoted phrase is kept whole.</div>` : "";
  return `<section class="rs-doc lit-results"><header><span>${results.searching ? "Searching for" : "Found for"} “${esc(results.query)}”</span>` +
    `<span class="rs-file">${results.searching ? "" : `${results.results.length} ${results.results.length === 1 ? "paper" : "papers"}`}</span>` +
    `<button class="lit-clear" data-lit-clear title="Put the results away" aria-label="Put the results away">${icon("close")}</button></header>` +
    `${failures}${list}${none}</section>`;
}

/** The papers the investigation keeps. @param {LiteratureEvent} lit @param {string} folder */
function papersHtml(lit, folder) {
  const where = `${folder}${lit.slug}/literature/`;
  const adding = lit.adding.length ? `<div class="rs-faint lit-adding">Looking up ${esc(and(lit.adding))}…</div>` : "";
  if (!lit.papers.length) {
    return `<section class="rs-doc"><header><span>Papers</span><code class="rs-file" title="${esc(where)}">literature/</code></header>${adding}
      <div class="cc-none rs-none">No papers yet. ${lit.enabled ? "Each round adds the best of what the team's queries find;" : "Turned on, each round adds the best of what the team's queries find;"}
      search above and add what you like, or type <code>/p</code> and a DOI or an arXiv id below. Each paper is a file in <code>${esc(where)}</code>,
      with its PDF beside it when an open copy is to be had.</div></section>`;
  }
  const rows = lit.papers.map((p) => {
    const by = p.by === "scientist" ? "you" : p.round != null ? `round ${p.round}` : "a round";
    const cited = p.cited ? `<span class="lit-badge" data-cited title="Cited in ${p.cited} of the team's reviews">cited ×${p.cited}</span>` : "";
    return `<button class="lit-row" data-lit-paper="${esc(p.id)}">` +
      `<span class="lit-id">${esc(p.id)}</span>` +
      `<span class="lit-name"><span class="lit-title">${esc(p.title)}</span><small>${esc(byline(p))}</small></span>` +
      `<span class="lit-badges">${pdfBadge(p, lit.fetching.includes(p.id))}${cited}<span class="lit-by">${esc(by)}</span></span></button>`;
  }).join("");
  return `<section class="rs-doc"><header><span>Papers</span><code class="rs-file" title="${esc(where)}">literature/</code></header>${adding}
    <div class="lit-table" role="list">${rows}</div></section>`;
}

/** The Literature tab, its list. @param {{literature: LiteratureEvent, results: ResultsEvent | null, folder: string, query: string, adding: string[]}} view */
export function literatureHtml({ literature, results, folder, query, adding }) {
  const now = Date.now() / 1000;
  return `<div class="rs-col rs-wide lit">` +
    headHtml(literature, now) +
    searchHtml(literature, query, Boolean(results?.searching)) +
    resultsHtml(results, adding, literature.enabled) +
    papersHtml(literature, folder) +
    `</div>`;
}

/** The host a URL names, or the URL when it names none. @param {string} url */
function hostOf(url) {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

/** A link to a paper elsewhere, opened in the browser. @param {string} url @param {string} label */
function link(url, label) {
  return `<a href="${esc(url)}" data-link title="${esc(url)}">${esc(label)}</a>`;
}

/** One paper, whole. @param {PaperDetail} d @param {{folder: string, slug: string, fetching: boolean, enabled: boolean}} where */
export function paperHtml(d, { folder, slug, fetching, enabled }) {
  const p = d.row;
  const authors = d.authors.length ? `${d.authors.join(", ")}${d.authorsMore ? ` and ${d.authorsMore} more` : ""}` : "";
  const by = p.by === "scientist" ? "added by you" : p.round != null ? `added in round ${p.round}` : "added by a round";
  const meta = [authors, p.year ? String(p.year) : "", p.venue ?? "", d.citations != null ? `cited ${d.citations.toLocaleString()} times` : "", by]
    .filter(Boolean).map(esc).join(" · ");
  const links = [
    d.doi ? link(`https://doi.org/${encodeURI(d.doi)}`, `doi:${d.doi}`) : "",
    d.arxiv ? link(`https://arxiv.org/abs/${encodeURI(d.arxiv)}`, `arXiv:${d.arxiv}`) : "",
    d.url && !/^https:\/\/(doi\.org|arxiv\.org\/abs)\//.test(d.url) ? link(d.url, "Its page") : "",
  ].filter(Boolean).join("");
  const path = `${folder}${slug}/literature/${p.id}.pdf`;
  const pdf = p.hasPdf
    ? `<div class="lit-pdf" data-pdf="kept">${icon("file-doc")}<span><b>${esc(p.id)}.pdf</b> is beside it${d.pdfFrom ? `, downloaded from ${link(d.pdfFrom, hostOf(d.pdfFrom))}` : ", put there by you"}.</span></div>`
    : `<div class="lit-pdf" data-pdf="none">${icon("file")}<span>${fetching ? "Its PDF is downloading…" : esc(d.pdfNote ?? "There is no PDF beside it.")}
        A copy you put at <code>${esc(path)}</code> is read like any other.</span>
        <button class="rs-link" data-lit-copy="${esc(path)}" title="Copy where it goes">Copy path</button></div>`;
  const action = p.hasPdf
    ? `<button class="cc-primary" data-lit-open="${esc(p.id)}">Open PDF</button>`
    : d.pdfUrl || d.arxiv ? `<button class="cc-quiet" data-lit-fetch="${esc(p.id)}"${fetching || !enabled ? " disabled" : ""}>${fetching ? "Downloading…" : "Download the PDF"}</button>` : "";
  const cited = d.citedBy.map((c) => `<li>${c.kind === "review" ? "Review" : "Hypothesis"} <b>${esc(c.by)}</b> of ` +
    `<button class="rs-link" data-research-hypothesis="${esc(c.hypothesis)}">${esc(c.hypothesis)}</button></li>`).join("");
  return `<div class="rs-col rs-detail lit-paper">
      <div class="rs-detail-bar"><button class="rs-back" data-lit-back>${icon("chev")}All papers</button><span class="grow"></span>${action}</div>
      <h2 class="rs-h"><span class="rs-id">${esc(p.id)}</span> ${esc(p.title)}</h2>
      <div class="rs-meta">${meta}</div>
      ${links ? `<div class="lit-links">${links}</div>` : ""}
      ${pdf}
      ${d.tldr.trim() ? `<section class="rs-doc"><header><span>In a line</span><span class="rs-file">Semantic Scholar's TL;DR</span></header><div class="cc-prose">${markdown(d.tldr)}</div></section>` : ""}
      <section class="rs-doc"><header><span>Abstract</span></header><div class="cc-prose">${d.abstract.trim() ? markdown(d.abstract) : "<p><em>The sources gave no abstract.</em></p>"}</div></section>
      ${cited ? `<section class="rs-doc"><header><span>Cited by the team</span></header><ul class="lit-cited">${cited}</ul></section>` : ""}
      ${d.queries.length ? `<section class="rs-doc"><header><span>Found by</span></header><p class="rs-faint">${d.queries.map((q) => `“${esc(q)}”`).join(", ")}</p></section>` : ""}
      ${d.extra.map(([heading, body]) => `<section class="rs-doc"><header><span>${esc(heading)}</span></header><div class="cc-prose">${markdown(body)}</div></section>`).join("")}
    </div>`;
}
