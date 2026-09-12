// Local layout preferences. Core mode/document state remains in app.js.
export function createPanels(root, changed) {
  const sections = [...root.querySelectorAll('.workspace')];
  const states = new Map();
  const minPane = 172, minStage = 320;
  let drag = null;
  const active = () => root.querySelector('.workspace.on');
  const key = (section) => `proton.panes.${section.dataset.ws}`;
  function save(section) {
    try { localStorage.setItem(key(section), JSON.stringify(states.get(section))); } catch { /* optional cache */ }
  }
  function paint(section) {
    const state = states.get(section);
    const panes = [...section.querySelectorAll(':scope > aside')];
    const hasRight = panes.length > 1;
    const fixed = section.dataset.ws === 'design' ? 66 : 0;
    const room = Math.max(0, section.clientWidth - fixed - minStage - 2);
    let left = !state.leftHidden, right = hasRight && !state.rightHidden;
    // Keep the user's choices; temporarily show only the most recently
    // requested side when both would crowd the stage.
    if (left && right && state['--col-l'] + state['--col-r'] > room) {
      if (state.preferred === 'right') left = false;
      else right = false;
    }
    const width = (side, visible) => visible ? Math.max(0, Math.min(state[`--col-${side}`], room)) : 0;
    section.style.setProperty('--col-l', `${width('l', left)}px`);
    section.style.setProperty('--col-r', `${width('r', right)}px`);
    section.style.gridTemplateColumns = `${fixed ? '66px ' : ''}${left ? 'var(--col-l) 1px' : '0px 0px'} minmax(0,1fr)${hasRight ? (right ? ' 1px var(--col-r)' : ' 0px 0px') : ''}`;
    panes.forEach((pane, index) => {
      const willHide = !(index === 0 ? left : right);
      if (willHide && pane.contains(root.activeElement)) {
        root.querySelector(`[data-panel-toggle="${index ? 'right' : 'left'}"]`)?.focus();
      }
      pane.hidden = willHide;
      pane.inert = pane.hidden;
    });
    section.querySelectorAll(':scope > .gutter').forEach((gutter) => {
      const isLeft = gutter.dataset.resize === '--col-l';
      gutter.hidden = !(isLeft ? left : right);
      gutter.setAttribute('aria-valuenow', String(Math.round(width(isLeft ? 'l' : 'r', !gutter.hidden))));
      gutter.setAttribute('aria-valuemax', String(Math.max(minPane, room)));
    });
    section.dataset.leftOpen = String(left);
    section.dataset.rightOpen = String(right);
    if (section === active()) {
      for (const [side, visible] of [['left', left], ['right', right]]) {
        const button = root.querySelector(`[data-panel-toggle="${side}"]`);
        button.disabled = side === 'right' && !hasRight;
        button.setAttribute('aria-expanded', String(visible));
        button.classList.toggle('on', visible);
        button.title = `${visible ? 'Hide' : 'Show'} ${side === 'left' ? 'sidebar' : 'right panel'}`;
        button.setAttribute('aria-label', button.title);
        const controlled = panes[side === 'left' ? 0 : 1];
        if (controlled) button.setAttribute('aria-controls', controlled.id);
        else button.removeAttribute('aria-controls');
      }
    }
  }
  function refresh() {
    const section = active();
    if (section) paint(section);
    changed();
  }
  function show(side, section = active()) {
    if (!section) return;
    const state = states.get(section);
    state[`${side}Hidden`] = false;
    state.preferred = side;
    save(section); refresh();
  }
  sections.forEach((section) => {
    const defaults = {
      '--col-l': parseFloat(section.style.getPropertyValue('--col-l')) || 232,
      '--col-r': parseFloat(section.style.getPropertyValue('--col-r')) || 280,
      leftHidden: false, rightHidden: false, preferred: 'left',
    };
    let saved;
    try { saved = JSON.parse(localStorage.getItem(key(section))); } catch { /* defaults */ }
    const state = { ...defaults };
    for (const prop of ['--col-l', '--col-r']) {
      if (Number.isFinite(saved?.[prop])) state[prop] = Math.max(minPane, Math.min(saved[prop], 800));
    }
    for (const prop of ['leftHidden', 'rightHidden']) if (typeof saved?.[prop] === 'boolean') state[prop] = saved[prop];
    if (saved?.preferred === 'right') state.preferred = 'right';
    states.set(section, state);
    // Explicit columns keep hidden children from shifting their neighbours.
    [...section.children].forEach((child, i) => { child.style.gridColumn = String(i + 1); child.style.gridRow = '1'; });
    section.querySelectorAll(':scope > aside').forEach((pane, i) => {
      pane.id ||= `${section.dataset.ws}-${i ? 'right' : 'left'}-panel`;
    });
    // Codex's dock already has an id used by its controller.
    section.querySelectorAll(':scope > .gutter').forEach((gutter) => {
      const prop = gutter.dataset.resize;
      gutter.tabIndex = 0;
      gutter.setAttribute('role', 'separator');
      gutter.setAttribute('aria-orientation', 'vertical');
      gutter.setAttribute('aria-label', `Resize ${prop === '--col-l' ? 'sidebar' : 'right panel'}`);
      gutter.setAttribute('aria-valuemin', String(minPane));
      gutter.title = 'Drag or use arrow keys to resize · double-click or Home to reset';
      gutter.addEventListener('pointerdown', (event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        drag = { section, gutter };
        root.documentElement.classList.add('resizing');
      });
      const reset = () => { state[prop] = defaults[prop]; save(section); refresh(); };
      gutter.addEventListener('dblclick', reset);
      gutter.addEventListener('keydown', (event) => {
        if (!['ArrowLeft', 'ArrowRight', 'Home'].includes(event.key)) return;
        event.preventDefault();
        if (event.key === 'Home') return reset();
        const direction = (event.key === 'ArrowRight' ? 1 : -1) * (prop === '--col-l' ? 1 : -1);
        state[prop] = Math.max(minPane, Math.min(800, state[prop] + direction * 20));
        save(section); refresh();
      });
    });
  });
  root.querySelectorAll('[data-panel-toggle]').forEach((button) => button.addEventListener('click', () => {
    const section = active(), side = button.dataset.panelToggle;
    const state = states.get(section);
    state[`${side}Hidden`] = section.dataset[`${side}Open`] === 'true';
    state.preferred = side;
    save(section); refresh();
  }));
  window.addEventListener('pointermove', (event) => {
    if (!drag) return;
    const { section, gutter } = drag;
    const box = section.getBoundingClientRect();
    const left = gutter.dataset.resize === '--col-l';
    const raw = left ? event.clientX - box.left - (section.dataset.ws === 'design' ? 66 : 0) : box.right - event.clientX;
    states.get(section)[gutter.dataset.resize] = Math.max(minPane, Math.min(800, Math.round(raw)));
    refresh();
  });
  const finish = () => {
    if (drag) save(drag.section);
    drag = null; root.documentElement.classList.remove('resizing');
  };
  for (const event of ['pointerup', 'pointercancel', 'blur']) window.addEventListener(event, finish);
  new ResizeObserver(refresh).observe(root.querySelector('.body'));
  return { refresh, show };
}
