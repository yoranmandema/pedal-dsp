// Class recipes shared by more than one component. Anything used once stays
// inline in its component; these exist so the repeats cannot drift apart.

/** The small square header button: preset step, presets, modal close. */
export const navBtn =
  "size-[var(--ui-touch)] rounded-key border border-line bg-raised-2 leading-none text-muted hover:border-line-hi hover:text-text";

/** A line of placeholder copy ("Select a block…", "No presets yet…"). */
export const emptyNote = "text-base text-muted";

/** A parameter's name above its control. */
export const ctrlName = "text-center text-2xs font-semibold tracking-[0.8px] text-muted uppercase";

/** A text field in a modal. */
export const textField =
  "rounded-md border border-amber-dim bg-inset text-md text-text outline-none focus:border-amber";

/** The modal backdrop, shell and its header, footer and error strip. */
export const modalBackdrop =
  "fixed inset-0 z-40 bg-[rgba(6,7,10,0.66)]";
export const modalShell =
  "relative z-[999] flex w-[460px] max-h-[calc(100vh-96px)] max-w-[calc(100vw-48px)] flex-col overflow-hidden rounded-xl border border-[#343743] bg-linear-to-b from-[#1e2027] to-well shadow-modal";
export const modalHead = "flex items-center justify-between gap-2 border-b border-line-soft px-3.5 py-3";
export const modalTitle = "text-base font-semibold tracking-[1.4px] text-muted uppercase";
export const modalFoot = "flex items-center gap-2 border-t border-line-soft px-3.5 py-3";