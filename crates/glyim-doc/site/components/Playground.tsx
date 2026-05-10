import React, { useState } from 'react';
import Editor from '@monaco-editor/react';
import { Button } from '@components/ui/button';
import { Play, Loader2 } from 'lucide-react';

export const Playground: React.FC<{ defaultCode?: string }> = ({ defaultCode = 'main = () => 42' }) => {
  const [code, setCode] = useState(defaultCode);
  const [output, setOutput] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const handleRun = async () => {
    setRunning(true);
    setOutput(null);
    try {
      // For now, simulate compilation (WASM integration pending)
      await new Promise(resolve => setTimeout(resolve, 500));
      setOutput('Compiled successfully. Exit code: 0');
    } catch (e) {
      setOutput(`Error: ${e}`);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="border rounded-lg overflow-hidden">
        <Editor
          height="300px"
          defaultLanguage="rust"
          value={code}
          onChange={(value) => setCode(value || '')}
          theme="vs-dark"
          options={{
            minimap: { enabled: false },
            fontSize: 14,
            lineNumbers: 'on',
          }}
        />
      </div>
      <div className="flex gap-2">
        <Button onClick={handleRun} disabled={running}>
          {running ? <Loader2 className="size-4 animate-spin" /> : <Play className="size-4" />}
          Run
        </Button>
      </div>
      {output && (
        <pre className="p-4 bg-muted border rounded-lg text-sm">{output}</pre>
      )}
    </div>
  );
};

export default Playground;
