'use strict';
const { cases, groups, states } = JSON.parse(document.querySelector('#gallery-data').textContent);
const $ = (selector) => document.querySelector(selector);
const filters = { group: 'all', state: 'all', theme: 'dark', text: 'normal', search: '' };
const variants = ['dark', 'light', 'dark-large', 'light-large'];
const variantLabel = (variant) => `${variant.startsWith('dark') ? 'Dark' : 'Light'} · ${variant.endsWith('large') ? '150%' : '100%'} text`;
const groupLabel = (group) => groups.find((item) => item.id === group)?.label || group;
const stateLabel = (state) => states.find((item) => item.id === state)?.label || state;
const element = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
const selectedVariants = () => variants.filter((variant) =>
  (filters.theme === 'both' || variant.startsWith(filters.theme)) &&
  (filters.text === 'both' || variant.endsWith('large') === (filters.text === 'large')));
const matches = (item) => (filters.state === 'all' || item.state === filters.state) &&
  filters.search.toLowerCase().trim().split(/\s+/).every((word) =>
    `${item.id} ${item.label} ${groupLabel(item.group)} ${stateLabel(item.state)}`.toLowerCase().includes(word));
let lastPreview;
let viewerCase;
let viewerVariant;
let viewerPage = 0;
const viewer = $('#viewer');

function showPage() {
  const focusedControl = document.activeElement;
  const pages = viewerCase.variants[viewerVariant].pages;
  viewerPage = Math.min(viewerPage, pages.length - 1);
  const page = pages[viewerPage];
  $('#viewer-image').src = page.file;
  $('#viewer-image').alt = `${viewerCase.label}, ${variantLabel(viewerVariant)}, scroll position ${viewerPage + 1}`;
  $('#original').href = page.file;
  $('#page-status').textContent = `${viewerPage + 1} / ${pages.length} · scroll ${page.scrollDp} dp`;
  $('#previous-page').disabled = viewerPage === 0;
  $('#next-page').disabled = viewerPage === pages.length - 1;
  if (viewer.open && focusedControl?.disabled) {
    const availableControl = !$('#previous-page').disabled ? $('#previous-page') :
      !$('#next-page').disabled ? $('#next-page') : $('#close-viewer');
    availableControl.focus({ preventScroll: true });
  }
  viewer.scrollTop = 0;
}
function openPreview(item, variant, button) {
  lastPreview = button;
  viewerCase = item;
  viewerVariant = variant;
  viewerPage = 0;
  $('#viewer-title').textContent = item.label;
  $('#viewer-group').textContent = `${groupLabel(item.group)} / ${stateLabel(item.state)}`;
  $('#viewer-variant').replaceChildren(...variants.map((value) => {
    const option = element('option', '', variantLabel(value)); option.value = value; return option;
  }));
  $('#viewer-variant').value = variant;
  showPage();
  viewer.showModal();
}
viewer.addEventListener('close', () => lastPreview?.focus({ preventScroll: true }));
$('#close-viewer').addEventListener('click', () => viewer.close());
$('#viewer-variant').addEventListener('change', (event) => { viewerVariant = event.target.value; showPage(); });
$('#previous-page').addEventListener('click', () => { viewerPage--; showPage(); });
$('#next-page').addEventListener('click', () => { viewerPage++; showPage(); });
viewer.addEventListener('keydown', (event) => {
  if (event.target.matches('select,option')) return;
  if (event.key === 'ArrowRight' && !$('#next-page').disabled) { event.preventDefault(); $('#next-page').click(); }
  if (event.key === 'ArrowLeft' && !$('#previous-page').disabled) { event.preventDefault(); $('#previous-page').click(); }
});

function card(item, shownVariants) {
  const article = element('article', 'case-card'); article.dataset.case = item.id;
  const heading = element('div', 'case-heading');
  heading.append(element('h3', '', item.title));
  const badge = element('span', 'badge', stateLabel(item.state)); badge.dataset.state = item.state;
  heading.append(badge); article.append(heading);
  const previews = element('div', 'previews');
  for (const variant of shownVariants) {
    const record = item.variants[variant];
    const preview = element('div', 'preview'); preview.dataset.variant = variant;
    preview.append(element('p', 'variant-label', variantLabel(variant)));
    const button = element('button', 'preview-button');
    button.setAttribute('aria-label', `Inspect ${item.label}, ${variantLabel(variant)}`);
    const image = element('img'); image.src = record.pages[0].file; image.alt = item.label;
    image.loading = 'lazy'; image.width = record.pages[0].widthPx; image.height = record.pages[0].heightPx;
    button.append(image); button.addEventListener('click', () => openPreview(item, variant, button));
    preview.append(button, element('p', 'scroll-hint', record.pages.length > 1 ? `${record.pages.length} scroll captures · inspect →` : 'Single screen · inspect →'));
    previews.append(preview);
  }
  article.append(previews, element('p', 'case-id', item.id));
  return article;
}
function render() {
  const base = cases.filter(matches);
  const visible = base.filter((item) => filters.group === 'all' || item.group === filters.group);
  const shownVariants = selectedVariants();
  $('#view-title').textContent = filters.group === 'all' ? 'All screens' : groupLabel(filters.group);
  $('#results').textContent = `${visible.length} of ${cases.length} screens · ${shownVariants.length === 1 ? variantLabel(shownVariants[0]) : `${shownVariants.length} variants per screen`}`;
  $('#comparison-note').textContent = shownVariants.length === 1 ? 'Click a preview to inspect' : 'Comparing variants · click to inspect';
  const navigation = [{ id: 'all', label: 'All screens' }, ...groups].map((group) => {
    const button = element('button'); button.dataset.group = group.id;
    button.setAttribute('aria-pressed', String(group.id === filters.group));
    button.append(element('span', '', group.label), element('span', 'count', String(base.filter((item) => group.id === 'all' || item.group === group.id).length)));
    button.addEventListener('click', () => { filters.group = group.id; render(); $('#groups [data-group="' + group.id + '"]').focus({ preventScroll: true }); window.scrollTo({ top: 0 }); });
    return button;
  });
  $('#groups').replaceChildren(...navigation);
  const sections = [];
  for (const group of groups) {
    const items = visible.filter((item) => item.group === group.id);
    if (!items.length) continue;
    const section = element('section', 'destination'); section.dataset.group = group.id;
    const title = element('h3', 'group-heading', group.label); title.append(element('span', '', `${items.length} screens`));
    const grid = element('div', 'case-grid'); grid.append(...items.map((item) => card(item, shownVariants)));
    section.append(title, grid); sections.push(section);
  }
  $('#gallery').dataset.comparing = String(shownVariants.length > 1);
  $('#gallery').style.setProperty('--variant-count', shownVariants.length);
  $('#gallery').replaceChildren(...sections);
  $('#empty').hidden = visible.length > 0;
}
for (const state of states) { const option = element('option', '', state.label); option.value = state.id; $('#state').append(option); }
for (const name of ['state', 'theme', 'text']) $('#' + name).addEventListener('change', (event) => { filters[name] = event.target.value; render(); });
$('#search').addEventListener('input', (event) => { filters.search = event.target.value; render(); });
function reset() {
  Object.assign(filters, { group: 'all', state: 'all', theme: 'dark', text: 'normal', search: '' });
  for (const name of ['search', 'state', 'theme', 'text']) $('#' + name).value = filters[name];
  render();
}
$('#reset').addEventListener('click', reset);
$('#empty-reset').addEventListener('click', reset);
render();
