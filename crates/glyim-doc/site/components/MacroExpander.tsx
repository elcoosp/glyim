import React, { useState } from 'react';
import { Button } from '@components/ui/button';

interface Props {
  original: string;
  expanded: string;
}

export const MacroExpander: React.FC<Props> = ({ original, expanded }) => {
  const [showExpanded, setShowExpanded] = useState(false);

  return (
    <div className="space-y-2">
      <div className="flex gap-2">
        <Button
          variant={showExpanded ? 'outline' : 'default'}
          size="sm"
          onClick={() => setShowExpanded(false)}
        >
          Original
        </Button>
        <Button
          variant={showExpanded ? 'default' : 'outline'}
          size="sm"
          onClick={() => setShowExpanded(true)}
        >
          Expanded
        </Button>
      </div>
      <pre className="p-4 bg-muted border rounded-lg text-sm overflow-x-auto">
        {showExpanded ? expanded : original}
      </pre>
    </div>
  );
};

export default MacroExpander;
