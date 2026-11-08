import { useState } from 'react';

export function RlePanel(props: { onImport: (text: string) => string | null; onExport: () => Promise<string> }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [message, setMessage] = useState<{ text: string; warn: boolean } | null>(null);

  const place = () => {
    const problem = props.onImport(text);
    setMessage(problem ? { text: problem, warn: true } : { text: 'Click the field to place it. Its own rule is now active, if it brought one.', warn: false });
  };
  const copyField = async () => {
    const exported = await props.onExport();
    const usable = exported.startsWith('x = ');
    setOpen(true);
    if (!usable) {
      setMessage({ text: exported, warn: true });
      return;
    }
    setText(exported);
    try {
      await navigator.clipboard.writeText(exported);
      setMessage({ text: 'Copied the whole field as RLE.', warn: false });
    } catch {
      setMessage({ text: 'Copy the text above, as the clipboard is not available here.', warn: false });
    }
  };

  return (
    <div className="rle">
      <span className="caption">Pattern as RLE text</span>
      <div className="chips">
        <button className={open ? 'chip active' : 'chip'} onMouseDown={(e) => e.preventDefault()} onClick={() => setOpen(!open)}>
          Paste RLE
        </button>
        <button className="chip" onMouseDown={(e) => e.preventDefault()} onClick={copyField}>
          Copy field as RLE
        </button>
      </div>
      {open && (
        <>
          <textarea className="text-input" rows={5} spellCheck={false} placeholder={'x = 3, y = 3, rule = B3/S23\nbob$2bo$3o!'} aria-label="Pattern in RLE format" value={text} onChange={(e) => setText(e.target.value)} />
          <div className="chips">
            <button className="chip" disabled={text.trim() === ''} onMouseDown={(e) => e.preventDefault()} onClick={place}>
              Place pattern
            </button>
          </div>
        </>
      )}
      {message && <p className={message.warn ? 'note warn' : 'note'}>{message.text}</p>}
    </div>
  );
}
