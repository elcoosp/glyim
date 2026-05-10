import React, { useEffect, useState } from 'react';
import type { DocManifest, DocItem } from '@lib/api';
import { HighlightedCode } from '@components/HighlightedCode';

export default function DocPage() {
  const [item, setItem] = useState<DocItem | null>(null);
  const slug = typeof window !== 'undefined'
    ? window.location.pathname.split('/doc/').pop()?.replace(/\/$/, '') || ''
    : '';

  useEffect(() => {
    if (!slug) return;
    fetch('/api/api.json')
      .then(res => res.json())
      .then((data: DocManifest) => {
        const found = data.items.find(i => i.qualified_name === slug);
        setItem(found || null);
      })
      .catch(() => setItem(null));
  }, [slug]);

  if (!item) return <div>Loading...</div>;

  return (
    <div style={{ padding: '2rem', maxWidth: '900px', margin: '0 auto' }}>
      <h1>{item.name}</h1>
      <div dangerouslySetInnerHTML={{ __html: item.signature_html }} />
      {item.doc && (
        <div style={{
          marginTop: '1rem',
          background: 'var(--rp-c-bg-soft)',
          padding: '1rem',
          borderRadius: '8px'
        }} dangerouslySetInnerHTML={{ __html: item.doc }} />
      )}
      <div style={{ marginTop: '2rem' }}>
        {item.highlighted_examples.map((ex, idx) => (
          <HighlightedCode key={idx} code={ex.code} html={ex.html} />
        ))}
      </div>
      <p style={{ marginTop: '2rem', color: 'var(--rp-c-text-3)' }}>
        <a href={`https://github.com/your-repo/blob/main/${item.source_file}#L${item.source_line}`}>
          [src]
        </a>
      </p>
    </div>
  );
}
