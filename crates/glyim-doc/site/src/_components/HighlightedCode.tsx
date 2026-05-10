import React from 'react';

interface Props {
  code: string;
  html: string;
}

export const HighlightedCode: React.FC<Props> = ({ code, html }) => {
  const handleCopy = () => navigator.clipboard.writeText(code);
  return React.createElement('div', { style: { position: 'relative', marginBottom: '1rem' } },
    React.createElement('button', {
      onClick: handleCopy,
      style: { position: 'absolute', top: '0.5rem', right: '0.5rem', cursor: 'pointer' },
      title: 'Copy code'
    }, '📋'),
    React.createElement('div', {
      dangerouslySetInnerHTML: { __html: html },
      style: { overflowX: 'auto', padding: '1rem', background: '#f4f4f4', borderRadius: '4px' }
    })
  );
};

export default HighlightedCode;
