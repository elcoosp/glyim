import React from 'react';
import { DocTestBadge } from './DocTestBadge';

interface DocTestResult {
  example_index: number;
  passed: boolean;
  output: string;
}

interface Props {
  code: string;
  html: string;
  testResult?: DocTestResult;
}

export const HighlightedCode: React.FC<Props> = ({ code, html, testResult }) => {
  const handleCopy = () => navigator.clipboard.writeText(code);
  return (
    <div style={{ position: 'relative', marginBottom: '1rem' }}>
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: '0.25rem' }}>
        <button
          onClick={handleCopy}
          style={{
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
        <DocTestBadge result={testResult} />
      </div>
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
