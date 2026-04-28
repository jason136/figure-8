declare class TextDecoder {
  constructor(label?: string);
  readonly encoding: string;
  decode(input?: ArrayBuffer | ArrayBufferView): string;
}

declare class TextEncoder {
  readonly encoding: string;
  encode(input?: string): Uint8Array;
  encodeInto(
    src: string,
    dest: Uint8Array,
  ): { read: number; written: number };
}

declare class Blob {
  constructor(
    parts?: (string | ArrayBuffer | ArrayBufferView | Blob)[],
    options?: { type?: string },
  );
  readonly size: number;
  readonly type: string;
  arrayBuffer(): Promise<ArrayBuffer>;
  text(): Promise<string>;
  slice(start?: number, end?: number, contentType?: string): Blob;
}

declare class Headers {
  constructor(
    init?: Headers | [string, string][] | Record<string, string>,
  );
  append(name: string, value: string): void;
  delete(name: string): void;
  get(name: string): string | null;
  has(name: string): boolean;
  set(name: string, value: string): void;
  forEach(
    cb: (value: string, key: string, parent: Headers) => void,
    thisArg?: unknown,
  ): void;
  entries(): IterableIterator<[string, string]>;
  keys(): IterableIterator<string>;
  values(): IterableIterator<string>;
  [Symbol.iterator](): IterableIterator<[string, string]>;
}

interface ResponseInit {
  status?: number;
  statusText?: string;
  headers?: Headers | [string, string][] | Record<string, string>;
}

declare class Response {
  constructor(
    body?: ArrayBuffer | ArrayBufferView | Blob | string | null,
    init?: ResponseInit,
  );
  readonly bodyUsed: boolean;
  readonly status: number;
  readonly statusText: string;
  readonly ok: boolean;
  readonly headers: Headers;
  readonly type: string;
  readonly url: string;
  readonly redirected: boolean;
  text(): Promise<string>;
  json(): Promise<unknown>;
  arrayBuffer(): Promise<ArrayBuffer>;
  blob(): Promise<Blob>;
  clone(): Response;
}

interface RequestInit {
  method?: string;
  headers?: Headers | [string, string][] | Record<string, string>;
  body?: string | ArrayBuffer | ArrayBufferView | Blob | null;
}

declare class Request {
  constructor(input: string | Request, init?: RequestInit);
  readonly url: string;
  readonly method: string;
  readonly headers: Headers;
  readonly body: unknown;
}

declare class FormData {
  constructor();
  append(name: string, value: string): void;
  delete(name: string): void;
  get(name: string): string | null;
  getAll(name: string): string[];
  has(name: string): boolean;
  set(name: string, value: string): void;
  entries(): IterableIterator<[string, string]>;
  keys(): IterableIterator<string>;
  values(): IterableIterator<string>;
  forEach(
    cb: (value: string, key: string, parent: FormData) => void,
    thisArg?: unknown,
  ): void;
  [Symbol.iterator](): IterableIterator<[string, string]>;
}

declare function fetch(
  resource: string | Request,
  init?: RequestInit,
): Promise<Response>;
