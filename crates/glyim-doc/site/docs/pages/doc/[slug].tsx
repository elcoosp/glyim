import React, { useEffect, useState } from 'react';
import type { DocManifest, DocItem } from '@lib/api';
import { HighlightedCode } from '@components/HighlightedCode';
import { SearchModal } from '@components/SearchModal';
import { Playground } from '@components/Playground';
import { Button } from '@components/ui/button';
import { Code2 } from 'lucide-react';

export default function DocPage() {
  const [item, setItem] = useState<DocItem | null>(null);
  const [showPlayground, setShowPlayground] = useState(false);
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
      <SearchModal />

      <div className="flex items-center justify-between mb-6">
        <h1 className="text-3xl font-bold">{item.name}</h1>
        <Button variant="outline" size="sm" onClick={() => setShowPlayground(!showPlayground)}>
          <Code2 className="size-4" />
          {showPlayground ? 'Hide Playground' : 'Try in Playground'}
        </Button>
      </div>

      {showPlayground && (
        <div className="mb-8 p-4 border rounded-lg">
          <h2 className="text-lg font-semibold mb-4">Interactive Playground</h2>
          <Playground defaultCode={item.highlighted_examples[0]?.code || 'main = () => 42'} />
        </div>
      )}

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
