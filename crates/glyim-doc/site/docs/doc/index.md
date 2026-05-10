---
title: API Documentation
description: Browse all documented Glyim items
---

# API Documentation

<div id="item-list">
  <p>Loading documented items... (enable JavaScript if this doesn't change)</p>
</div>

<script>
  fetch('/api/api.json')
    .then(res => res.json())
    .then(data => {
      const list = document.getElementById('item-list');
      if (!data.items || !data.items.length) {
        list.innerHTML = '<p>No documented items found. Run <code>glyim doc</code> to generate.</p>';
        return;
      }
      list.innerHTML = '<ul style="list-style:none;padding:0">' + data.items.map(item =>
        '<li style="margin-bottom:0.5rem">' +
          '<a href="/doc/' + encodeURIComponent(item.qualified_name) + '.html" style="font-weight:600">' +
            item.name +
          '</a> <code style="font-size:0.85rem;color:var(--rp-c-text-2)">' + item.kind + '</code>' +
          (item.doc ? '<p style="margin:0;color:var(--rp-c-text-3);font-size:0.85rem">' + item.doc.substring(0, 120) + '…</p>' : '') +
        '</li>'
      ).join('') + '</ul>';
    })
    .catch(() => {
      document.getElementById('item-list').innerHTML =
        '<p>Could not load API data. Ensure you built the documentation with <code>glyim doc</code> and reload the page.</p>';
    });
</script>
