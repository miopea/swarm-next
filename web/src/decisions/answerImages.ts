/**
 * Images the operator sends with an answer or a question (ADR 0174).
 *
 * The worker's model takes an image of at most 3,932,160 bytes (5 MiB once
 * base64-encoded) and 8,000 pixels a side, so a larger one is shrunk here,
 * before it is sent, rather than refused after the operator has written their
 * answer around it.
 */

/** The largest image Swarm keeps for a request. Matches the server. */
export const MAX_ANSWER_IMAGE_BYTES = 3_932_160;
/** Images in one answer or question. Matches the server. */
export const MAX_ANSWER_IMAGES = 4;
/** What a worker's model can be shown, so what the server accepts. */
export const ANSWER_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;
/** A side longer than this is scaled down. Under the model's 8,000. */
const MAX_SIDE = 7680;

export type DraftImage = {
  /** Stable within this draft; not sent. */
  key: string;
  name: string;
  mediaType: string;
  /** Base64, without a data: prefix. */
  data: string;
  bytes: number;
};

/** What the server takes, per image. */
export type SentImage = { media_type: string; data: string };

export function toSentImages(images: readonly DraftImage[]): SentImage[] {
  return images.map((image) => ({ media_type: image.mediaType, data: image.data }));
}

export function previewUrl(image: DraftImage): string {
  return `data:${image.mediaType};base64,${image.data}`;
}

/** The image files in a paste, in the order they were copied. */
export function imageFiles(files: FileList | readonly File[] | null | undefined): File[] {
  return [...(files ?? [])].filter((file) => file.type.startsWith("image/"));
}

/** Whether an image can go as it is, or has to be redrawn smaller first. */
export function needsShrinking(mediaType: string, bytes: number, width?: number, height?: number): boolean {
  if (!(ANSWER_IMAGE_TYPES as readonly string[]).includes(mediaType)) return true;
  if (bytes > MAX_ANSWER_IMAGE_BYTES) return true;
  return Math.max(width ?? 0, height ?? 0) > MAX_SIDE;
}

export class ImageNotUsable extends Error {}

/**
 * Reads one image into a draft, shrinking it when it is too large or in a
 * type the worker cannot be shown.
 */
export async function prepareImage(file: File): Promise<DraftImage> {
  if (!file.type.startsWith("image/")) throw new ImageNotUsable(`${file.name || "That file"} is not an image.`);
  let blob: Blob = file;
  let mediaType = file.type;
  const size = await dimensions(file);
  if (needsShrinking(file.type, file.size, size?.width, size?.height)) {
    blob = await shrink(file, size);
    mediaType = blob.type;
  }
  return {
    key: crypto.randomUUID(),
    name: file.name || "Pasted image",
    mediaType,
    data: await base64(blob),
    bytes: blob.size,
  };
}

async function dimensions(file: Blob): Promise<{ width: number; height: number } | undefined> {
  if (typeof createImageBitmap !== "function") return undefined;
  try {
    const bitmap = await createImageBitmap(file);
    const size = { width: bitmap.width, height: bitmap.height };
    bitmap.close();
    return size;
  } catch {
    return undefined;
  }
}

/**
 * Redraws an image as JPEG, scaling down until it fits. Five tries, each 30%
 * smaller, is enough for any screenshot a display produces; past that the
 * image is refused with a reason rather than sent blurred beyond reading.
 */
async function shrink(file: Blob, size: { width: number; height: number } | undefined): Promise<Blob> {
  if (!size || typeof createImageBitmap !== "function") {
    throw new ImageNotUsable("This image could not be read. Try a PNG or JPEG screenshot.");
  }
  const bitmap = await createImageBitmap(file);
  try {
    let scale = Math.min(1, MAX_SIDE / Math.max(size.width, size.height));
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(size.width * scale));
      canvas.height = Math.max(1, Math.round(size.height * scale));
      const context = canvas.getContext("2d");
      if (!context) break;
      // JPEG has no transparency; white is what a screenshot's clear pixels
      // were drawn over.
      context.fillStyle = "#ffffff";
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.85));
      if (blob && blob.size <= MAX_ANSWER_IMAGE_BYTES) return blob;
      scale *= 0.7;
    }
  } finally {
    bitmap.close();
  }
  throw new ImageNotUsable("This image is too large to send, even made smaller. Try a smaller screenshot.");
}

function base64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new ImageNotUsable("This image could not be read."));
    reader.onload = () => {
      const url = String(reader.result);
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.readAsDataURL(blob);
  });
}
