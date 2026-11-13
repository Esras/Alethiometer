// textarea-caret (component/textarea-caret-position) ships no types; this is its one export.
declare module 'textarea-caret' {
  export default function getCaretCoordinates(
    element: HTMLTextAreaElement | HTMLInputElement,
    position: number,
    options?: { debug?: boolean },
  ): { top: number; left: number; height: number };
}
