import { useEffect, useId, useRef, useState, type ClipboardEvent, type ReactNode } from "react";

import type { DecisionImage } from "../api";
import { imageFiles, ImageNotUsable, prepareImage, previewUrl, type DraftImage } from "./answerImages";

type Props = {
  images: readonly DraftImage[];
  /** Given an update, never a value: two pastes read at once both land. */
  onChange: (update: (current: DraftImage[]) => DraftImage[]) => void;
  /** How many this field may hold, after any sharing the same answer. */
  limit: number;
  disabled?: boolean;
  /** The text field. A paste into it lands here. */
  children: ReactNode;
};

/**
 * Screenshots going with one answer field (ADR 0174). Wraps the field so a
 * paste into it is caught, and offers a picker beside it for a file.
 */
export default function AnswerImages({ images, onChange, limit, disabled = false, children }: Props) {
  const input = useRef<HTMLInputElement>(null);
  const hintId = useId();
  const [error, setError] = useState<string>();
  const [reading, setReading] = useState(0);
  const room = Math.max(0, limit - images.length);

  async function add(files: File[]) {
    if (!files.length || disabled) return;
    const taken = files.slice(0, room);
    setError(files.length > taken.length
      ? taken.length === 0 ? "No more screenshots fit with this answer." : `Only ${taken.length} more fit with this answer; the rest were left out.`
      : undefined);
    if (!taken.length) return;
    setReading((count) => count + taken.length);
    const prepared: DraftImage[] = [];
    for (const file of taken) {
      try {
        prepared.push(await prepareImage(file));
      } catch (failure) {
        setError(failure instanceof ImageNotUsable ? failure.message : "This image could not be read.");
      } finally {
        setReading((count) => count - 1);
      }
    }
    if (prepared.length) onChange((current) => [...current, ...prepared].slice(0, limit));
  }

  function pasted(event: ClipboardEvent<HTMLElement>) {
    const files = imageFiles(event.clipboardData.files);
    if (!files.length) return;
    // A copied web image can carry its address as text too. The image is what
    // was meant; text alone pastes as it always did.
    if (!event.clipboardData.getData("text/plain")) event.preventDefault();
    void add(files);
  }

  return (
    <div className="answer-images" onPaste={pasted}>
      {children}
      <div className="answer-images-controls">
        <button type="button" className="text-button" disabled={disabled || room === 0} aria-describedby={hintId} onClick={() => input.current?.click()}>
          Attach screenshot
        </button>
        <small id={hintId} className="muted">
          {room === 0 ? "No more fit with this answer." : "Or paste one into the box."}
        </small>
        <input ref={input} type="file" accept="image/*" multiple hidden tabIndex={-1}
          onChange={(event) => { void add(imageFiles(event.target.files)); event.target.value = ""; }} />
      </div>
      {reading > 0 && <p role="status" className="muted">Reading the image…</p>}
      {error && <p role="alert" className="field-error">{error}</p>}
      {images.length > 0 && (
        <ul className="answer-image-list" aria-label="Screenshots to send">
          {images.map((image, index) => (
            <li key={image.key}>
              <img src={previewUrl(image)} alt={`Screenshot ${index + 1}: ${image.name}`} />
              <button type="button" className="text-button" disabled={disabled}
                aria-label={`Remove screenshot ${index + 1}`}
                onClick={() => { setError(undefined); onChange((current) => current.filter((held) => held.key !== image.key)); }}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export type FetchDecisionImage = (decisionId: string, imageId: string, signal: AbortSignal) => Promise<Blob>;

/** Screenshots already sent, read back from the Hive to show what went. */
export function SentImages({ images, onFetchImage, label }: {
  images: readonly DecisionImage[];
  onFetchImage?: FetchDecisionImage;
  label: string;
}) {
  const [urls, setUrls] = useState<Record<string, string>>({});
  const [failed, setFailed] = useState(false);
  // Read by identity, not by array: a refresh of the same images is not a
  // new set to fetch.
  const identity = images.map((image) => image.id).join(",");
  const latest = useRef(images);
  latest.current = images;

  useEffect(() => {
    const wanted = latest.current;
    if (!onFetchImage || !wanted.length) return;
    const request = new AbortController();
    const made: string[] = [];
    setFailed(false);
    void Promise.all(wanted.map(async (image) => {
      const blob = await onFetchImage(image.decision_id, image.id, request.signal);
      const url = URL.createObjectURL(blob);
      made.push(url);
      return [image.id, url] as const;
    })).then((loaded) => {
      if (!request.signal.aborted) setUrls(Object.fromEntries(loaded));
    }, () => {
      if (!request.signal.aborted) setFailed(true);
    });
    return () => {
      request.abort();
      for (const url of made) URL.revokeObjectURL(url);
    };
  }, [identity, onFetchImage]);

  if (!images.length) return null;
  const count = `${images.length} ${images.length === 1 ? "screenshot" : "screenshots"}`;
  if (!onFetchImage || failed) return <p className="muted sent-images">{count} sent{failed ? " · could not be shown here" : ""}</p>;
  return (
    <ul className="answer-image-list sent-images" aria-label={label}>
      {images.map((image, index) => (
        <li key={image.id}>
          {urls[image.id]
            ? <a href={urls[image.id]} target="_blank" rel="noreferrer"><img src={urls[image.id]} alt={`Sent screenshot ${index + 1}`} /></a>
            : <span className="answer-image-loading" role="status">Loading screenshot {index + 1}…</span>}
        </li>
      ))}
    </ul>
  );
}
