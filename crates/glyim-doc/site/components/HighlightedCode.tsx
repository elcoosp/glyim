import React from 'react';
import { Button } from '@components/ui/button';
import { Copy } from 'lucide-react';

interface Props {
  code: string;
  html: string;
}

export const HighlightedCode: React.FC<Props> = ({ code, html }) => {
  const handleCopy = () => navigator.clipboard.writeText(code);
  return (
    <div className="relative">
      <Button variant="ghost" size="sm" onClick={handleCopy} className="absolute top-2 right-2">
        <Copy className="size-4" />
        Copy
      </Button>
      <pre
        dangerouslySetInnerHTML={{ __html: html }}
        className="overflow-x-auto p-4 bg-muted border rounded-lg text-sm"
      />
    </div>
  );
};

export default HighlightedCode;
