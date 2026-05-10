import React, { useEffect } from 'react';
import '@pagefind/component-ui/css';

export const SearchModal: React.FC = () => {
  useEffect(() => {
    // Dynamically import the web component
    import('@pagefind/component-ui');
  }, []);

  return (
    <>
      {/* Hidden trigger – activated by ⌘K */}
      <pagefind-modal-trigger style={{ display: 'none' }} />
      <pagefind-modal />
    </>
  );
};

export default SearchModal;
