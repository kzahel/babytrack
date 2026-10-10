<script>
  // Line icons drawn on a 24px grid; color comes from the surrounding text.
  // Placeholder artwork until a final icon set is chosen.
  let { name, size = 24, label = '' } = $props();
  const paths = {
    bottle: '<path d="M10 2.5h4M10.5 2.5v2.8L8.4 8.2v11.6a1.7 1.7 0 0 0 1.7 1.7h3.8a1.7 1.7 0 0 0 1.7-1.7V8.2l-2.1-2.9V2.5"/><path d="M8.4 12h3M8.4 15.5h3"/>',
    breast: '<path d="M12 20.5s-7.5-4.4-7.5-10.1A4.2 4.2 0 0 1 12 8a4.2 4.2 0 0 1 7.5 2.4c0 5.7-7.5 10.1-7.5 10.1z"/>',
    pump: '<path d="M7 4h10l-1.5 5h-7z"/><path d="M9 9v3a3 3 0 0 0 6 0V9"/><path d="M10.5 15v6h3v-6"/>',
    solids: '<path d="M4 11h16a8 8 0 0 1-16 0z"/><path d="M9 4c0 2 2 2 2 4M14 4c0 2 2 2 2 4"/>',
    sleep: '<path d="M19.5 14.6A7.8 7.8 0 1 1 9.4 4.5a6.3 6.3 0 0 0 10.1 10.1z"/>',
    diaper: '<path d="M3.5 7.5h17v2.6a8.5 8.5 0 0 1-17 0z"/><path d="M8 7.5l1.5 4M16 7.5l-1.5 4"/>',
    wet: '<path d="M12 3.2s5.8 6.3 5.8 10.6a5.8 5.8 0 0 1-11.6 0C6.2 9.5 12 3.2 12 3.2z"/><path d="M9.3 14.5a2.8 2.8 0 0 0 2.4 2.6"/>',
    dirty: '<path d="M3.5 7.5h17v2.6a8.5 8.5 0 0 1-17 0z"/><path d="M8 7.5l1.5 4M16 7.5l-1.5 4"/><circle cx="12" cy="13.6" r="1.4" fill="currentColor" stroke="none"/>',
    growth: '<rect x="3" y="8" width="18" height="8" rx="2"/><path d="M7 8v3M11 8v4M15 8v3M19 8v2"/>',
    temperature: '<path d="M10 14V5a2 2 0 0 1 4 0v9a4 4 0 1 1-4 0z"/><path d="M12 10v6"/>',
    medication: '<rect x="3.5" y="8.5" width="17" height="7" rx="3.5" transform="rotate(-45 12 12)"/><path d="M9.5 9.5l5 5"/>',
    note: '<path d="M5 19l1-4L16 5l3 3L9 18z"/><path d="M14 7l3 3"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    chevron: '<path d="M9 5l7 7-7 7"/>',
    lock: '<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>',
    phone: '<rect x="6" y="2" width="12" height="20" rx="3"/><path d="M10 18h4"/>',
    'no-account': '<circle cx="12" cy="8" r="4"/><path d="M4 21c0-4 3.6-7 8-7s8 3 8 7"/><path d="M3 3l18 18"/>',
    mark: '<circle cx="12" cy="12" r="6.5" stroke-width="2.6"/><circle cx="12" cy="12" r="2" fill="currentColor" stroke="none"/>',
  };
</script>

<span class="icon" style:width="{size}px" style:height="{size}px" role={label ? 'img' : undefined}
  aria-label={label || undefined} aria-hidden={label ? undefined : 'true'}>
  <svg viewBox="0 0 24 24">{@html paths[name] || paths.note}</svg>
</span>
