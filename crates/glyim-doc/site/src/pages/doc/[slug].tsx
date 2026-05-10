import React, { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'rspress/runtime';
import type { DocManifest, DocItem } from '../../_lib/api';
import { HighlightedCode } from '../../_components/HighlightedCode';

export default function DocPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const slug = location.pathname.split('/doc/').pop() || '';
  const [item, setItem] = useState<DocItem | null>(null);

  useEffect(() => {
    fetch('/api/api.json')
      .then(res => res.json())
      .then((data: DocManifest) => {
        const found = data.items.find(i => i.qualified_name === slug);
        if (found) setItem(found);
        else navigate('/404');
      })
      .catch(() => navigate('/'));
  }, [slug]);

  if (!item) return <div>Loading...</div>;

  return (
    <div style={{ padding: '2rem', maxWidth: '900px', margin: '0 auto' }}>
      <h1>{item.name}</h1>
      <div dangerouslySetInnerHTML={{ __html: item.signature_html }} />
      {item.doc && (
        <div style={{ marginTop: '1rem', background: '#f8f8f8', padding: '1rem', borderRadius: '4px' }}
             dangerouslySetInnerHTML={{ __html: item.doc }} />
      )}
      <div style={{ marginTop: '2rem' }}>
        {item.highlighted_examples.map((ex, idx) => (
          <HighlightedCode key={idx} code={ex.code} html={ex.html} />
        ))}
      </div>
      <p style={{ marginTop: '2rem', color: '#888' }}>
        <a href={`https://github.com/your-repo/blob/main/${item.source_file}#L${item.source_line}`}>[src]</a>
      </p>
    </div>
  );
}
