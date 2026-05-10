import React, { useEffect, useState } from 'react';
import { SearchModal } from '@components/SearchModal';

interface DocItem {
  name: string;
  kind: string;
  qualified_name: string;
  doc: string | null;
  highlighted_examples: { code: string; html: string; hash: string }[];
}

interface DocManifest {
  items: DocItem[];
}

export default function DocIndex() {
  const [items, setItems] = useState<DocItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch('/api/api.json')
      .then(res => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((data: DocManifest) => {
        setItems(data.items || []);
        setLoading(false);
      })
      .catch(err => {
        setError(err.message);
        setLoading(false);
      });
  }, []);

  if (loading) {
    return (
      <div className="max-w-4xl mx-auto p-8">
        <SearchModal />
        <h1 className="text-3xl font-bold mb-6">API Documentation</h1>
        <p className="text-muted-foreground">Loading documented items…</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="max-w-4xl mx-auto p-8">
        <SearchModal />
        <h1 className="text-3xl font-bold mb-6">API Documentation</h1>
        <div className="bg-destructive/10 border border-destructive/20 rounded-lg p-4">
          <p className="font-semibold">Could not load API data.</p>
          <p className="text-sm mt-2">Error: {error}</p>
          <p className="text-sm mt-2">
            Make sure you're viewing this through <code>pnpm dev</code> or <code>pnpm preview</code>,
            not by opening the HTML file directly.
          </p>
        </div>
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div className="max-w-4xl mx-auto p-8">
        <SearchModal />
        <h1 className="text-3xl font-bold mb-6">API Documentation</h1>
        <p className="text-muted-foreground">
          No documented items found. Run <code>glyim doc</code> to generate documentation.
        </p>
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto p-8">
      <SearchModal />
      <h1 className="text-3xl font-bold mb-6">API Documentation</h1>
      <p className="text-muted-foreground mb-6">{items.length} documented items</p>
      <ul className="space-y-3">
        {items.map(item => (
          <li key={item.qualified_name} className="border-l-3 border-primary bg-muted/50 p-4 rounded-r-lg">
            <a
              href={`/doc/${encodeURIComponent(item.qualified_name)}`}
              className="text-primary hover:underline font-semibold text-lg"
            >
              {item.name}
            </a>
            {' '}
            <code className="text-xs bg-background px-2 py-0.5 rounded text-muted-foreground">
              {item.kind}
            </code>
            {item.doc && (
              <p className="text-sm text-muted-foreground mt-1">
                {item.doc.length > 150 ? item.doc.substring(0, 150) + '…' : item.doc}
              </p>
            )}
            {item.highlighted_examples.length > 0 && (
              <p className="text-xs text-muted-foreground mt-1">
                📝 {item.highlighted_examples.length} code example(s)
              </p>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
