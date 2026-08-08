import { canonicalize, publicKeyFingerprint, sha256Hex, type FragmentEnvelope } from "./protocol";

const DB_NAME = "proofline-evidence-v2";
const KEY_STORE = "keys";
const QUEUE_STORE = "upload-queue";

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 2);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(KEY_STORE)) database.createObjectStore(KEY_STORE);
      if (!database.objectStoreNames.contains(QUEUE_STORE)) database.createObjectStore(QUEUE_STORE, { keyPath: "id" });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function transactionResult<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export function toBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export async function exportSpki(key: CryptoKey): Promise<string> {
  return toBase64Url(new Uint8Array(await crypto.subtle.exportKey("spki", key)));
}

export async function getOrCreateBrowserIdentity(): Promise<CryptoKeyPair & { spki: string; fingerprint: string }> {
  const database = await openDatabase();
  const transaction = database.transaction(KEY_STORE, "readwrite");
  let pair = await transactionResult(transaction.objectStore(KEY_STORE).get("device-identity")) as CryptoKeyPair | undefined;
  if (!pair) {
    pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
    transaction.objectStore(KEY_STORE).put(pair, "device-identity");
  }
  const spki = await exportSpki(pair.publicKey);
  return { ...pair, spki, fingerprint: await publicKeyFingerprint(spki) };
}

export async function createSessionKey(): Promise<CryptoKeyPair & { spki: string }> {
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  return { ...pair, spki: await exportSpki(pair.publicKey) };
}

export async function signCanonical(privateKey: CryptoKey, value: unknown): Promise<string> {
  const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, privateKey, new TextEncoder().encode(canonicalize(value)));
  return toBase64Url(new Uint8Array(signature));
}

export async function buildFragmentEnvelope(input: Omit<FragmentEnvelope, "protocol_version" | "media_digest" | "chain_digest" | "byte_length">, bytes: ArrayBuffer): Promise<FragmentEnvelope> {
  const media_digest = await sha256Hex(bytes);
  const base = { protocol_version: "proofline/2" as const, ...input, media_digest, byte_length: bytes.byteLength };
  return { ...base, chain_digest: await sha256Hex(canonicalize(base)) };
}

interface QueuedFragment {
  id: string;
  captureId: string;
  streamId: string;
  sequence: number;
  mediaBaseUrl: string;
  uploadToken: string;
  envelope: FragmentEnvelope;
  signature: string;
  blob: Blob;
  queuedAt: string;
}

export async function queueFragment(fragment: QueuedFragment): Promise<void> {
  const database = await openDatabase();
  const transaction = database.transaction(QUEUE_STORE, "readwrite");
  transaction.objectStore(QUEUE_STORE).put(fragment);
  await new Promise<void>((resolve, reject) => { transaction.oncomplete = () => resolve(); transaction.onerror = () => reject(transaction.error); });
}

async function removeQueued(id: string): Promise<void> {
  const database = await openDatabase();
  const transaction = database.transaction(QUEUE_STORE, "readwrite");
  transaction.objectStore(QUEUE_STORE).delete(id);
  await new Promise<void>((resolve, reject) => { transaction.oncomplete = () => resolve(); transaction.onerror = () => reject(transaction.error); });
}

export async function uploadQueuedFragment(fragment: QueuedFragment): Promise<Record<string, unknown>> {
  const envelope = toBase64Url(new TextEncoder().encode(canonicalize(fragment.envelope)));
  const response = await fetch(`${fragment.mediaBaseUrl.replace(/\/$/, "")}/ingest/v1/${fragment.captureId}/${fragment.streamId}/${fragment.sequence}`, {
    method: "PUT",
    headers: {
      authorization: `Bearer ${fragment.uploadToken}`,
      "content-type": fragment.blob.type || "application/octet-stream",
      "x-proofline-envelope": envelope,
      "x-proofline-signature": fragment.signature,
    },
    body: fragment.blob,
  });
  if (!response.ok) throw new Error(`Relay rejected fragment ${fragment.sequence} (${response.status})`);
  const receipt = await response.json() as Record<string, unknown>;
  await removeQueued(fragment.id);
  return receipt;
}

export async function drainUploadQueue(): Promise<number> {
  const database = await openDatabase();
  const transaction = database.transaction(QUEUE_STORE, "readonly");
  const queued = await transactionResult(transaction.objectStore(QUEUE_STORE).getAll()) as QueuedFragment[];
  let uploaded = 0;
  for (const fragment of queued.sort((a, b) => a.queuedAt.localeCompare(b.queuedAt))) {
    try { await uploadQueuedFragment(fragment); uploaded += 1; } catch { break; }
  }
  return uploaded;
}
