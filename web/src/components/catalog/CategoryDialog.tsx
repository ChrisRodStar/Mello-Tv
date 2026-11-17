import { useEffect, useId, useRef, useState } from 'react';

import { loadShelf } from '../../lib/catalog';
import type { Title } from '../../model';
import type { Category } from '../../types';
import { Icon } from '../common/Icon';
import { TitleCard } from './TitleCard';

export function CategoryDialog({
  profileId,
  category,
  shelf,
  onClose,
  onSelect,
}: {
  profileId: string;
  category: Category;
  shelf: { id: string; heading: string };
  onClose: () => void;
  onSelect: (title: Title) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const headingId = useId();
  const [titles, setTitles] = useState<Title[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(30_000)]);
    void loadShelf(profileId, category, shelf.id, signal).then(
      (result) => {
        if (!controller.signal.aborted) setTitles(result);
      },
      () => {
        if (!controller.signal.aborted) setFailed(true);
      },
    );
    return () => controller.abort();
  }, [attempt, category, profileId, shelf.id]);

  return (
    <dialog
      ref={dialog}
      className="category-dialog"
      aria-labelledby={headingId}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <header className="category-dialog-header">
        <h2 id={headingId}>{shelf.heading}</h2>
        <button type="button" className="shelf-view-more" onClick={onClose}>
          <Icon name="close" /> Close
        </button>
      </header>
      {failed ? (
        <div className="category-dialog-status" role="alert">
          Couldn’t load this category.{' '}
          <button
            type="button"
            className="shelf-view-more"
            onClick={() => {
              setFailed(false);
              setAttempt((value) => value + 1);
            }}
          >
            Try again
          </button>
        </div>
      ) : titles === null ? (
        <p className="category-dialog-status" role="status">Loading titles…</p>
      ) : titles.length === 0 ? (
        <p className="category-dialog-status">No titles are available in this category.</p>
      ) : (
        <div className="category-grid">
          {titles.map((title) => (
            <div key={`${title.type}:${title.id}`} className="category-grid-item">
              <TitleCard title={title} onSelect={onSelect} />
              <span aria-hidden="true">{title.title}</span>
            </div>
          ))}
        </div>
      )}
    </dialog>
  );
}
