import '@/docs/styles/tailwind.css'
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

  if (!item) return <div className="p-8 text-center text-muted-foreground">Loading...</div>;

  return (
    <div className="max-w-4xl mx-auto p-8">
      <h1 className="text-3xl font-bold mb-6">{item.name}</h1>
      <div dangerouslySetInnerHTML={{ __html: item.signature_html }} className="mb-6" />
      {item.doc && (
        <div className="bg-muted p-6 rounded-lg mb-8"
             dangerouslySetInnerHTML={{ __html: item.doc }} />
      )}
      <div className="space-y-6">
        {item.highlighted_examples.map((ex, idx) => (
          <HighlightedCode key={idx} code={ex.code} html={ex.html} />
        ))}
      </div>
      <p className="mt-8 text-sm text-muted-foreground">
        <a href={`https://github.com/your-repo/blob/main/${item.source_file}#L${item.source_line}`}
           className="text-primary hover:underline">
          [src]
        </a>
      </p>
    </div>
  );
}
