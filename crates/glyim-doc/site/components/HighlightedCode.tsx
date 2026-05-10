import React from 'react';

interface Props {
  code: string;
  html: string;
}

export const HighlightedCode: React.FC<Props> = ({ code, html }) => {
  const handleCopy = () => navigator.clipboard.writeText(code);
  return (
    <div style={{ position: 'relative', marginBottom: '1rem' }}>
      <button
        onClick={handleCopy}
        style={{
          position: 'absolute',
          top: '0.5rem',
          right: '0.5rem',
          cursor: 'pointer',
          background: 'var(--rp-c-bg-soft)',
          border: '1px solid var(--rp-c-border)',
          borderRadius: '4px',
          padding: '0.25rem 0.5rem',
          fontSize: '0.75rem'
        }}
        title="Copy code"
      >
        📋 Copy
      </button>
      <pre
        dangerouslySetInnerHTML={{ __html: html }}
        style={{
          overflowX: 'auto',
          padding: '1rem',
          background: 'var(--rp-c-bg-soft)',
          border: '1px solid var(--rp-c-border)',
          borderRadius: '8px'
        }}
      />
    </div>
  );
};

export default HighlightedCode;
