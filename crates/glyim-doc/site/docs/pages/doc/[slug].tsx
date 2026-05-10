import React, { useEffect, useState } from 'react';
import type { DocManifest, DocItem } from '@lib/api';
import { HighlightedCode } from '@components/HighlightedCode';
import { SearchModal } from '@components/SearchModal';
import { Playground } from '@components/Playground';
import { Button } from '@components/ui/button';
import { Code2 } from 'lucide-react';

export default function DocPage() {
  const [items, setItems] = useState<DocManifest | null>(null);
  const [item, setItem] = useState<DocItem | null>(null);
  const [loading, setLoading] = useState(true);
  const slug = typeof window !== 'undefined'
    ? window.location.pathname.replace(/^\/doc\//, '').replace(/\/$/, '')
    : '';

  useEffect(() => {
    fetch('/api/api.json')
      .then(res => res.json())
      .then((data: DocManifest) => {
        setItems(data);
        const found = data.items.find(
          i => i.qualified_name === slug || i.name === slug
        );
        setItem(found || null);
        setLoading(false);
      })
      .catch(() => {
        setLoading(false);
      });
  }, [slug]);

  if (loading) return <div className="p-8 text-center text-muted-foreground">Loading...</div>;

  // If no slug, show listing
  if (!slug && items) {
    return (
      <div className="max-w-4xl mx-auto p-8">
        <SearchModal />
        <h1 className="text-3xl font-bold mb-6">API Documentation</h1>
        <ul className="space-y-2">
          {items.items.map(i => (
            <li key={i.qualified_name}>
              <a href={`/doc/${encodeURIComponent(i.qualified_name)}`}
                 className="text-primary hover:underline font-medium">
                {i.name}
              </a>
              {' '}
              <code className="text-sm text-muted-foreground">{i.kind}</code>
              {i.doc && <p className="text-sm text-muted-foreground mt-1">{i.doc.substring(0, 100)}...</p>}
            </li>
          ))}
        </ul>
      </div>
    );
  }

  if (!item) {
    return (
      <div className="max-w-4xl mx-auto p-8">
        <SearchModal />
        <div className="text-center py-16">
          <h1 className="text-2xl font-bold mb-4">Item not found</h1>
          <p className="text-muted-foreground">
            The documentation for &quot;{slug}&quot; could not be found.
          </p>
          <a href="/doc/index.html" className="text-primary hover:underline mt-4 inline-block">
            ← Back to documentation index
          </a>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto p-8">
      <SearchModal />

      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-3xl font-bold">{item.name}</h1>
          <p className="text-sm text-muted-foreground">{item.kind} · {item.qualified_name}</p>
        </div>
        <Button variant="outline" size="sm" onClick={() => {
          const el = document.getElementById('playground');
          if (el) el.classList.toggle('hidden');
        }}>
          <Code2 className="size-4" />
          Playground
        </Button>
      </div>

      <div id="playground" className="hidden mb-8 p-4 border rounded-lg">
        <h2 className="text-lg font-semibold mb-4">Interactive Playground</h2>
        <Playground defaultCode={item.highlighted_examples[0]?.code || 'main = () => 42'} />
      </div>

      <div className="bg-card border rounded-lg p-6 mb-6 font-mono text-sm">
        <div dangerouslySetInnerHTML={{ __html: item.signature_html }} />
      </div>

      {item.doc && (
        <div className="bg-muted p-6 rounded-lg mb-8"
             dangerouslySetInnerHTML={{ __html: item.doc }} />
      )}

      <div className="space-y-6">
        {item.highlighted_examples.map((ex, idx) => (
          <div key={idx}>
            <HighlightedCode code={ex.code} html={ex.html} />
          </div>
        ))}
      </div>

      <div className="mt-8 pt-4 border-t flex justify-between text-sm text-muted-foreground">
        <a href={`https://github.com/elcoosp/glyim/blob/main/${item.source_file}#L${item.source_line}`}
           className="text-primary hover:underline">
          [src] {item.source_file}:{item.source_line}
        </a>
        {items && (
          <a href="/doc/index.html" className="text-primary hover:underline">
            ← Back to index
          </a>
        )}
      </div>
    </div>
  );
}
