export interface Attachment {
  name: string;
  mimeType: string;
  size: number;
  dataBase64: string;
  isImage: boolean;
  previewUrl?: string;
}

export interface SavedAttachment {
  id: string;
  name: string;
  mime_type: string;
  size: number;
  path: string;
  is_image: boolean;
}

export const MAX_ATTACHMENTS = 5;
export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;
export const LARGE_TEXT_CHARS = 4000;
export const LARGE_TEXT_LINES = 80;

const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "heic", "heif"]);

function fileExtension(name: string): string {
  const idx = name.lastIndexOf(".");
  return idx >= 0 ? name.slice(idx + 1).toLowerCase() : "";
}

export function isImageFile(file: File): boolean {
  return file.type.startsWith("image/") || IMAGE_EXTENSIONS.has(fileExtension(file.name));
}

function isHeicFile(file: File): boolean {
  const mime = file.type.toLowerCase();
  const ext = fileExtension(file.name);
  return mime === "image/heic" || mime === "image/heif" || ext === "heic" || ext === "heif";
}

function readAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : "");
    reader.onerror = () => reject(new Error("读取附件失败。"));
    reader.readAsDataURL(blob);
  });
}

async function convertHeicToJpeg(file: File): Promise<{ blob: Blob; name: string; mimeType: string }> {
  try {
    const bitmap = await createImageBitmap(file);
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      bitmap.close();
      return { blob: file, name: file.name, mimeType: file.type || "image/heic" };
    }
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    const jpegBlob = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, "image/jpeg", 0.92));
    if (!jpegBlob) {
      return { blob: file, name: file.name, mimeType: file.type || "image/heic" };
    }
    const baseName = file.name.replace(/\.(heic|heif)$/i, "") || "image";
    return { blob: jpegBlob, name: `${baseName}.jpg`, mimeType: "image/jpeg" };
  } catch {
    return { blob: file, name: file.name, mimeType: file.type || "image/heic" };
  }
}

export async function prepareAttachments(incoming: File[], existingCount: number): Promise<Attachment[]> {
  if (existingCount + incoming.length > MAX_ATTACHMENTS) {
    throw new Error(`一次最多添加 ${MAX_ATTACHMENTS} 个附件。`);
  }
  const results: Attachment[] = [];
  for (const rawFile of incoming) {
    if (rawFile.size > MAX_ATTACHMENT_BYTES) {
      throw new Error(`附件「${rawFile.name}」超过 10 MB 限制。`);
    }
    const image = isImageFile(rawFile);
    let blob: Blob = rawFile;
    let name = rawFile.name || (image ? "pasted-image.png" : "attachment");
    let mimeType = rawFile.type || (image ? "image/png" : "application/octet-stream");

    if (image && isHeicFile(rawFile)) {
      const converted = await convertHeicToJpeg(rawFile);
      blob = converted.blob;
      name = converted.name;
      mimeType = converted.mimeType;
      if (blob.size > MAX_ATTACHMENT_BYTES) {
        throw new Error(`附件「${name}」超过 10 MB 限制。`);
      }
    }

    const dataUrl = await readAsDataUrl(blob);
    const commaIdx = dataUrl.indexOf(",");
    const dataBase64 = commaIdx >= 0 ? dataUrl.slice(commaIdx + 1) : dataUrl;
    results.push({
      name,
      mimeType,
      size: blob.size,
      dataBase64,
      isImage: image,
      previewUrl: image ? `data:${mimeType};base64,${dataBase64}` : undefined,
    });
  }
  return results;
}

export function createTextPasteAttachment(text: string, index: number): Attachment {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > MAX_ATTACHMENT_BYTES) {
    throw new Error("粘贴的文本超过 10 MB 限制。");
  }
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
  }
  return {
    name: `pasted-text-${index}.txt`,
    mimeType: "text/plain",
    size: bytes.length,
    dataBase64: btoa(binary),
    isImage: false,
  };
}

export function isLargeTextPaste(text: string): boolean {
  return text.length >= LARGE_TEXT_CHARS || text.split(/\r?\n/).length >= LARGE_TEXT_LINES;
}

export function formatAttachmentSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
