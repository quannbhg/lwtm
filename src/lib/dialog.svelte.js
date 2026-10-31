// Shared confirmation dialog (replaces the browser's confirm() so warnings can have multiple levels).

export const dialog = $state({ open: false, title: "", message: "", details: [], level: null, okLabel: "OK", requireAck: false });

let resolver = null;

/** @returns {Promise<boolean>} */
export function confirmDialog({ title, message, details = [], level = null, okLabel = "Đồng ý", requireAck = false }) {
  resolver?.(false);
  Object.assign(dialog, { open: true, title, message, details, level, okLabel, requireAck });
  return new Promise((resolve) => (resolver = resolve));
}

export function closeDialog(result) {
  dialog.open = false;
  resolver?.(result);
  resolver = null;
}
