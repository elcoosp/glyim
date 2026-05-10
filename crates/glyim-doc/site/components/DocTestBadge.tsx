import React from 'react';

interface DocTestResult {
  example_index: number;
  passed: boolean;
  output: string;
}

export const DocTestBadge: React.FC<{ result?: DocTestResult }> = ({ result }) => {
  if (!result) return null;
  return (
    <span style={{
      display: 'inline-block',
      padding: '0.15rem 0.5rem',
      borderRadius: '4px',
      fontSize: '0.75rem',
      marginLeft: '0.5rem',
      background: result.passed ? 'var(--rp-c-green-light)' : 'var(--rp-c-red-light)',
      color: result.passed ? 'var(--rp-c-green)' : 'var(--rp-c-red)',
      border: `1px solid ${result.passed ? 'var(--rp-c-green)' : 'var(--rp-c-red)'}`
    }}>
      {result.passed ? '✓ pass' : '✗ fail'}
    </span>
  );
};

export default DocTestBadge;
