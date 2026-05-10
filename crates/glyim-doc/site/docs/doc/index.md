---
title: API Documentation
description: Browse all documented Glyim items
---

# API Documentation

<div id="item-list">
  <p>Loading documented items…</p>
</div>

<script>
(async () => {
  const list = document.getElementById('item-list');
  try {
    const res = await fetch('/api/api.json');
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const data = await res.json();
    if (!data.items || !data.items.length) {
      list.innerHTML = '<p>No documented items found. Run <code>glyim doc</code> to generate them.</p>';
      return;
    }
    list.innerHTML = '<ul style="list-style:none;padding:0">' + data.items.map(item =>
      '<li style="margin-bottom:0.8rem;padding:0.5rem;border-left:3px solid var(--rp-c-brand);background:var(--rp-c-bg-soft)">' +
        '<a href="/doc/' + encodeURIComponent(item.qualified_name) + '" style="font-weight:600;font-size:1.1rem">' +
          item.name +
        '</a> ' +
        '<code style="font-size:0.8rem;color:var(--rp-c-text-2);background:var(--rp-c-bg);padding:2px 6px;border-radius:3px">' + item.kind + '</code>' +
        (item.doc ? '<p style="margin:0.3rem 0 0;color:var(--rp-c-text-3);font-size:0.85rem">' + item.doc.substring(0, 150) + '…</p>' : '') +
        (item.highlighted_examples.length > 0 ? '<p style="margin:0.3rem 0 0;color:var(--rp-c-text-2);font-size:0.8rem">📝 ' + item.highlighted_examples.length + ' code example(s)</p>' : '') +
      '</li>'
    ).join('') + '</ul>';
  } catch (err) {
    list.innerHTML = '<div style="background:var(--rp-c-bg-soft);padding:1rem;border-radius:8px">' +
      '<p><strong>Could not load API data.</strong></p>' +
      '<p>This happens if you opened the HTML file directly from your file system.</p>' +
      '<p>To view the documentation correctly:</p>' +
      '<pre style="background:var(--rp-c-bg);padding:1rem;border-radius:4px;overflow-x:auto">' +
      'cd crates/glyim-doc/site\n' +
      'pnpm dev          # development server\n' +
      '# or\n' +
      'pnpm preview     # preview built site</pre>' +
      '<p>Error details: <code>' + err.message + '</code></p>' +
      '</div>';
  }
})();
</script>
