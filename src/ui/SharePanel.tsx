import { useRef, useState } from 'react';
import { linkFromImport, LONG_LINK_LENGTH, type LinkContent } from '../share/link';

const NOTES: Record<LinkContent, { text: string; warn: boolean }> = {
  exact: { text: 'Opens this field as it is now, paused, with the generation count back at 0.', warn: false },
  start: { text: 'Too many cells for a link, so this one holds the random start of this run instead of the current field.', warn: true },
  empty: { text: 'The field is empty, so this link only holds the settings.', warn: false },
  'too-many': { text: 'Too many cells for a link, so this one only holds the settings.', warn: true },
  'hat-cells': { text: 'Cells of an extended ruleset cannot go into a link yet, so this one only holds the settings.', warn: true },
};

export function SharePanel(props: { problems: string[]; create: () => Promise<{ url: string; content: LinkContent }> }) {
  const [link, setLink] = useState<{ url: string; content: LinkContent } | null>(null);
  const [copy, setCopy] = useState<'idle' | 'done' | 'manual'>('idle');
  const inputRef = useRef<HTMLInputElement>(null);
  const [importProblem, setImportProblem] = useState(false);

  /** Opening means reloading the page with that fragment, like any other link. */
  const open = (text: string) => {
    const hash = linkFromImport(text);
    setImportProblem(hash === null);
    if (hash === null) return;
    if (window.location.hash === hash) window.location.reload();
    else window.location.hash = hash;
  };

  const create = async () => {
    const created = await props.create();
    setLink(created);
    try {
      // Only available on https and localhost.
      await navigator.clipboard.writeText(created.url);
      setCopy('done');
    } catch {
      setCopy('manual');
      requestAnimationFrame(() => inputRef.current?.select());
    }
  };

  return (
    <>
      {props.problems.length > 0 && <p className="note warn">The link you opened could not be used completely. Ignored: {[...new Set(props.problems)].join(', ')}.</p>}
      <p className="note">A link holds the tile, the rule, the view and the cells. Nothing is sent anywhere, it all sits in the link itself.</p>
      <div className="chips">
        <button className="chip" onMouseDown={(e) => e.preventDefault()} onClick={create}>
          {link ? 'Update link' : 'Create link'}
        </button>
      </div>
      {link && (
        <>
          <input ref={inputRef} className="text-input" type="text" readOnly value={link.url} aria-label="Link to this field" onFocus={(e) => e.target.select()} />
          <p className="note">
            {copy === 'done' ? 'Copied. ' : 'Press Ctrl+C to copy. '}
            {link.url.length.toLocaleString('en-US')} characters.
          </p>
          <p className={NOTES[link.content].warn ? 'note warn' : 'note'}>{NOTES[link.content].text}</p>
          {link.url.length > LONG_LINK_LENGTH && <p className="note warn">Some messengers and mail programs cut off links of this length.</p>}
        </>
      )}
      <label className="chip file-chip">
        Open a find file
        <input
          type="file"
          accept=".json,.txt"
          onChange={async (e) => {
            const file = e.target.files?.[0];
            if (file) open(await file.text());
            e.target.value = '';
          }}
        />
      </label>
      {importProblem && <p className="note warn">This file holds no link to a field of this site.</p>}
    </>
  );
}
