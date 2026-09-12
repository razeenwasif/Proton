/** Safe in both text and quoted HTML attributes. @param {unknown} value */
export function escapeHtml(value) {
  /** @type {Record<string, string>} */
  const entities = {'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'};
  return String(value).replace(/[&<>"']/g, char => entities[char]);
}

/** @param {string} id @param {string} [cls] */
export function icon(id, cls = '') {
  return `<svg class="i${cls ? ' ' + escapeHtml(cls) : ''}"><use href="#i-${escapeHtml(id)}"/></svg>`;
}
