import React from 'react';

interface Props {
  typeName: string;
  documentation?: string;
}

export const TypeTooltip: React.FC<Props> = ({ typeName, documentation }) => {
  return (
    <span className="relative group">
      <span className="underline decoration-dotted cursor-help">{typeName}</span>
      <div className="absolute z-50 hidden group-hover:block bg-popover text-popover-foreground p-2 rounded-lg border shadow-lg text-sm max-w-xs">
        {documentation || `Type: ${typeName}`}
      </div>
    </span>
  );
};

export default TypeTooltip;
