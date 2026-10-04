// Settings rendered from the core's project settings, without write-back echoes.
/**
 * @param {Document} root
 * @param {(key: string, value: unknown) => void} keep
 * @param {() => void} changed
 */
export function createDisplaySettings(root, keep, changed) {
  /** @type {Record<string, string | number>} */
  const defaults = { 'ui.textScale': 1, 'ui.density': 'comfortable', 'ui.blur': 46, 'editor.fontSize': 16, 'editor.tabs': 'hidden', 'terminal.renderer': 'text' };
  /** @type {Record<string, readonly (string | number)[]>} */
  const choices = { 'ui.textScale': [1, 1.1, 1.25], 'ui.density': ['comfortable', 'compact'], 'ui.blur': [0, 20, 46], 'editor.fontSize': [12, 13, 14, 15, 16, 18, 20], 'editor.tabs': ['hidden', 'shown'], 'terminal.renderer': ['text', 'gpu'] };
  const inputs = [...root.querySelectorAll('[data-display-setting]')].filter((node) => node instanceof HTMLSelectElement);
  let current = { ...defaults };
  function paint() {
    const style = root.documentElement.style;
    style.setProperty('--ui-scale', String(current['ui.textScale']));
    style.setProperty('--editor-font-size', `${current['editor.fontSize']}px`);
    style.setProperty('--glass-float', `saturate(200%) blur(${current['ui.blur']}px)`);
    root.documentElement.dataset.density = String(current['ui.density']);
    root.documentElement.dataset.tabs = String(current['editor.tabs']);
    root.documentElement.dataset.termRenderer = String(current['terminal.renderer']);
    root.documentElement.dataset.surfaceBlur = String(current['ui.blur']);
    inputs.forEach((input) => { input.value = String(current[input.dataset.displaySetting ?? '']); });
    changed();
  }
  inputs.forEach((input) => input.addEventListener('change', () => {
    const key = input.dataset.displaySetting ?? '';
    if (!(key in defaults)) return;
    const value = typeof defaults[key] === 'number' ? Number(input.value) : input.value;
    if (!choices[key].includes(value)) return;
    current[key] = value; paint(); keep(key, value);
  }));
  return {
    /** @param {unknown} values */
    apply(values) {
    current = { ...defaults };
    for (const key of Object.keys(defaults)) {
      const value = key.split('.').reduce((obj, part) => obj && typeof obj === 'object' ? /** @type {Record<string, unknown>} */ (obj)[part] : undefined, values);
      if ((typeof value === 'number' || typeof value === 'string') && choices[key].includes(value)) current[key] = value;
    }
    paint();
  } };
}
