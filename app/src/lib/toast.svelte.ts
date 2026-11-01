// Minimal toast store. Used for error notices and short confirmations.
export type ToastKind = 'error' | 'success' | 'info';
export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

export const toasts = $state<Toast[]>([]);
let next = 1;

export function dismissToast(id: number): void {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

export function toast(kind: ToastKind, text: string, ms = kind === 'error' ? 7000 : 3500): void {
  const id = next++;
  toasts.push({ id, kind, text });
  setTimeout(() => dismissToast(id), ms);
}

export function toastError(e: unknown): void {
  console.error(e);
  toast('error', String(e instanceof Error ? e.message : e));
}
