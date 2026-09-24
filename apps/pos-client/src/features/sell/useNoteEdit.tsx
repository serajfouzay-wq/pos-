import type { Uuid } from '@pos/shared';
import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import type { CartLine } from './lines';

function NoteDialog({
  line,
  onClose,
  onSave,
}: {
  line: CartLine;
  onClose: () => void;
  onSave: (note: string | null) => void;
}) {
  const { t } = useTranslation();
  const [note, setNote] = useState(line.note ?? '');
  return (
    <Modal open title={line.name} onClose={onClose}>
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          onSave(note.trim() || null);
        }}
      >
        <label className="field">
          <span>{t('options.note')}</span>
          <input
            autoFocus
            maxLength={200}
            value={note}
            placeholder={t('options.notePlaceholder')}
            onChange={(e) => {
              setNote(e.target.value);
            }}
          />
        </label>
        <button type="submit" className="button button--primary button--block">
          {t('common.save')}
        </button>
      </form>
    </Modal>
  );
}

/** A kitchen note on a line not yet sent ("no onions"). */
export function useNoteEdit(onSet: (lineId: Uuid, note: string | null) => void): {
  edit: (line: CartLine) => void;
  dialog: ReactNode;
  busy: boolean;
} {
  const [line, setLine] = useState<CartLine | null>(null);
  const dialog = line && (
    <NoteDialog
      key={line.line_id}
      line={line}
      onClose={() => {
        setLine(null);
      }}
      onSave={(note) => {
        onSet(line.line_id, note);
        setLine(null);
      }}
    />
  );
  return { edit: setLine, dialog, busy: line !== null };
}
