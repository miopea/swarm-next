import type { ReactNode } from "react";
import { createPortal } from "react-dom";

/** Blocking overlays belong to the document, not a paint-contained workspace.
 * Keep focus/draft ownership in the dialog; only its DOM placement changes.
 * No extra container or retained stack is needed. */
export default function ModalPortal({ children }: { children: ReactNode }) {
  return createPortal(children, document.body);
}
