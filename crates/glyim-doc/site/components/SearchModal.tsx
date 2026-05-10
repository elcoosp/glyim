import React, { useEffect } from 'react';
import { Button } from '@components/ui/button';
import { Search } from 'lucide-react';

export const SearchModal: React.FC = () => {
  useEffect(() => {
    import('@pagefind/component-ui');
  }, []);

  return (
    <>
      <Button
        variant="outline"
        className="fixed top-4 right-4 z-50 gap-2"
        onClick={() => document.querySelector<HTMLElement>('pagefind-modal-trigger')?.click()}
      >
        <Search className="size-4" />
        Search ⌘K
      </Button>
      <pagefind-modal-trigger style={{ display: 'none' }} />
      <pagefind-modal />
    </>
  );
};

export default SearchModal;
