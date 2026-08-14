export function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

export function downloadJson(value: unknown, filename: string) {
  downloadBlob(
    new Blob([`${JSON.stringify(value, null, 2)}\n`], { type: "application/json" }),
    filename,
  );
}
