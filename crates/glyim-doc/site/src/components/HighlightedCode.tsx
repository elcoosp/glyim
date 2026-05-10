import React from 'react';
import { Copy } from 'lucide-react';

export function HighlightedCode({ code, html }: { code: string; html: string }) {
  const handleCopy = () => {
    navigator.clipboard.writeText(code);
  };

  return (
    <div style={{ position: 'relative', marginBottom: '1rem' }}>
      <button
        onClick={handleCopy}
        style={{ position: 'absolute', top: '0.5rem', right: '0.5rem', background: '#eee', border: 'none', cursor: 'pointer', borderRadius: '4px', padding: '0.25rem 0.5rem' }}
        title="Copy code"
      >
        <Copy size={14} />
      </button>
      <div dangerouslySetInnerHTML={{ __html: html }} style={{ overflowX: 'auto', padding: '1rem', background: '#f4f4f4', borderRadius: '4px' }} />
    </div>
  );
}
