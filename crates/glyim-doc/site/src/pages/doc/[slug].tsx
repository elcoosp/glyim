import React, { useEffect, useState } from 'react';
import { useRouter } from 'rspress/runtime';
import { DocManifest, DocItem } from '../../lib/api';
import { HighlightedCode } from '../../components/HighlightedCode';

export default function DocPage() {
  const router = useRouter();
  const slug = router.query.slug as string;
  const [item, setItem] = useState<DocItem | null>(null);
  const [manifest, setManifest] = useState<DocManifest | null>(null);

  useEffect(() => {
    fetch('/api/api.json')
      .then(res => res.json())
      .then((data: DocManifest) => {
        setManifest(data);
        const found = data.items.find(i => i.qualified_name === slug);
        if (found) setItem(found);
      });
  }, [slug]);

  if (!manifest) return <div>Loading...</div>;
  if (!item) return <div>Item not found: {slug}</div>;

  return (
    <div style={{ padding: '2rem', maxWidth: '900px', margin: '0 auto' }}>
      <h1>{item.name}</h1>
      <div dangerouslySetInnerHTML={{ __html: item.signature_html }} />
      {item.doc && (
        <div style={{ marginTop: '1rem', background: '#f8f8f8', padding: '1rem', borderRadius: '4px' }}>
          <div dangerouslySetInnerHTML={{ __html: item.doc }} />
        </div>
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
