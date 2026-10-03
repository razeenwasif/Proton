/** @typedef {import('./bindings/TreeRow').TreeRow} TreeRow */
/** @typedef {import('./protocol').Dispatch} Dispatch */

/**
 * The core supplies the visible rows and owns expansion. This controller owns
 * keyboard focus only; moving focus never opens or edits a file.
 * @param {Document} root
 * @param {Dispatch} dispatch
 * @param {(name: string) => string} icon Trusted, app-owned SVG markup.
 * @param {(open: boolean) => string} folder The same, for a folder.
 */
export function createFileTree(root, dispatch, icon, folder) {
  const targets = [...root.querySelectorAll('[data-tree]')].filter((node) => node instanceof HTMLElement);
  /** @type {TreeRow[]} */
  let rows = [];
  /** @type {WeakMap<HTMLElement, string>} */
  const focused = new WeakMap();
  /** @param {HTMLElement} target */
  const items = (target) => [...target.querySelectorAll('[role="treeitem"]')].filter((node) => node instanceof HTMLElement);
  /** @param {string} text */
  const escape = (text) => text.replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
  /** @param {HTMLElement} target @param {string} path @param {boolean} [move] */
  function focus(target, path, move = true) {
    focused.set(target, path);
    for (const item of items(target)) {
      item.tabIndex = item.dataset.treePath === path ? 0 : -1;
      if (move && item.tabIndex === 0) {
        item.focus({ preventScroll: true });
        item.scrollIntoView({ block: 'nearest' });
      }
    }
  }
  /** @param {TreeRow} row */
  function activate(row) {
    if (row.dir) dispatch({ type: 'toggleTreeNode', path: row.path });
    else dispatch({ type: 'openBuffer', path: row.path });
  }
  for (const target of targets) {
    target.setAttribute('role', 'tree');
    target.setAttribute('aria-label', 'Project files');
    let prefix = '', typedAt = 0;
    target.addEventListener('click', (event) => {
      const item = event.target instanceof Element ? event.target.closest('[data-tree-path]') : null;
      if (!(item instanceof HTMLElement)) return;
      const row = rows.find((row) => row.path === item.dataset.treePath && !row.truncated);
      if (!row) return;
      focus(target, row.path);
      activate(row);
    });
    target.addEventListener('focusin', (event) => {
      if (event.target instanceof HTMLElement && event.target.dataset.treePath) {
        focus(target, event.target.dataset.treePath, false);
      }
    });
    target.addEventListener('keydown', (event) => {
      if (event.metaKey || event.ctrlKey || event.altKey || event.key === 'Tab') return;
      const visible = rows.filter((row) => !row.truncated);
      const index = visible.findIndex((row) => row.path === focused.get(target));
      const row = visible[index];
      if (!row) return;
      event.preventDefault();
      event.stopPropagation();
      let next = index;
      switch (event.key) {
        case 'ArrowDown': next = Math.min(visible.length - 1, index + 1); break;
        case 'ArrowUp': next = Math.max(0, index - 1); break;
        case 'Home': next = 0; break;
        case 'End': next = visible.length - 1; break;
        case 'ArrowRight':
          if (row.dir && !row.expanded) activate(row);
          else if (row.dir && visible[index + 1]?.depth > row.depth) next++;
          break;
        case 'ArrowLeft':
          if (row.dir && row.expanded) activate(row);
          else {
            for (let i = index - 1; i >= 0; i--) {
              if (visible[i].depth < row.depth) { next = i; break; }
            }
          }
          break;
        case 'Enter': case ' ': activate(row); break;
        case 'Escape': {
          const editor = target.closest('.workspace')?.querySelector('[data-editor]');
          if (editor instanceof HTMLElement) { if (!editor.hasAttribute('tabindex')) editor.tabIndex = -1; editor.focus(); }
          return;
        }
        default:
          if (event.key.length !== 1 || event.isComposing) return;
          prefix = Date.now() - typedAt > 700 ? event.key.toLowerCase() : prefix + event.key.toLowerCase();
          typedAt = Date.now();
          for (let step = 1; step <= visible.length; step++) {
            const candidate = (index + step) % visible.length;
            if (visible[candidate].name.toLowerCase().startsWith(prefix)) { next = candidate; break; }
          }
      }
      // A synchronous preview transport can already have rebuilt the tree.
      focus(target, visible[next].path);
    });
  }
  return {
    /** @param {TreeRow[]} incoming */
    render(incoming) {
      rows = incoming;
      const visible = rows.filter((row) => !row.truncated);
      // Flat DOM, explicit hierarchy for assistive technology.
      /** @type {Map<string, TreeRow[]>} */
      const groups = new Map();
      for (const row of visible) {
        const parent = row.path.slice(0, row.path.lastIndexOf('/') + 1);
        const siblings = groups.get(parent) ?? [];
        siblings.push(row); groups.set(parent, siblings);
      }
      const html = rows.map((row) => {
        // The depth, not a width: how far a level indents is the stylesheet's
        // to say, and Codex says it differently from the other modes.
        const padding = `--depth:${row.depth}`;
        if (row.truncated) return `<div class="row more" role="none" style="${padding}">${escape(row.name)}</div>`;
        const siblings = groups.get(row.path.slice(0, row.path.lastIndexOf('/') + 1)) ?? [];
        const image = row.dir
          ? `<span class="ic"><svg class="i chev"><use href="#i-chev"/></svg></span><span class="fic">${folder(row.expanded)}</span>`
          : `<span class="ic">${icon(row.name)}</span>`;
        return `<div class="row${row.dir ? ' dir' : ''}${row.expanded ? ' open' : ''}${row.active ? ' sel' : ''}" role="treeitem" tabindex="-1"` +
          ` aria-label="${escape(row.name)}" aria-level="${row.depth + 1}" aria-posinset="${siblings.indexOf(row) + 1}" aria-setsize="${siblings.length}"` +
          ` aria-selected="${row.active}"${row.dir ? ` aria-expanded="${row.expanded}"` : ''}` +
          ` data-tree-path="${escape(row.path)}" style="${padding}"><span aria-hidden="true">${image}</span><span>${escape(row.name)}</span></div>`;
      }).join('');
      for (const target of targets) {
        const hadFocus = target.contains(root.activeElement);
        const previous = focused.get(target);
        let chosen = previous;
        while (chosen && !visible.some((row) => row.path === chosen)) {
          const at = chosen.lastIndexOf('/');
          chosen = at < 0 ? '' : chosen.slice(0, at);
        }
        chosen = chosen || visible.find((row) => row.active)?.path || visible[0]?.path;
        target.innerHTML = html || '<div class="empty">No project files</div>';
        if (chosen) focus(target, chosen, hadFocus);
        if (!hadFocus && target.offsetParent) target.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
      }
    },
  };
}
