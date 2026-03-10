{
  const __raw = globalThis.__rawFetch;
  const __decode = globalThis.__utf8Decode;
  const __encode = globalThis.__utf8Encode;
  delete globalThis.__rawFetch;
  delete globalThis.__utf8Decode;
  delete globalThis.__utf8Encode;

  class TextDecoder {
    #encoding;
    constructor(label) {
      const enc = (label || 'utf-8').toLowerCase().replace(/[^a-z0-9]/g, '');
      if (enc !== 'utf8')
        throw new RangeError('only UTF-8 encoding is supported');
      this.#encoding = 'utf-8';
    }
    get encoding() {
      return this.#encoding;
    }
    decode(input) {
      if (!input) return '';
      if (input instanceof ArrayBuffer || ArrayBuffer.isView(input))
        return __decode(input);
      throw new TypeError('expected ArrayBuffer or TypedArray');
    }
  }

  class TextEncoder {
    get encoding() {
      return 'utf-8';
    }
    encode(input) {
      return new Uint8Array(__encode(String(input !== undefined ? input : '')));
    }
    encodeInto(src, dest) {
      const encoded = this.encode(src);
      const written = Math.min(encoded.length, dest.length);
      dest.set(encoded.subarray(0, written));
      return { read: src.length, written };
    }
  }

  class Blob {
    #buffer;
    #type;
    constructor(parts, options) {
      this.#type = (options && options.type) || '';
      if (!parts || parts.length === 0) {
        this.#buffer = new ArrayBuffer(0);
        return;
      }
      const chunks = [];
      let total = 0;
      for (const part of parts) {
        let bytes;
        if (part instanceof ArrayBuffer) bytes = new Uint8Array(part);
        else if (part instanceof Blob) bytes = new Uint8Array(part.#buffer);
        else if (ArrayBuffer.isView(part))
          bytes = new Uint8Array(part.buffer, part.byteOffset, part.byteLength);
        else bytes = new Uint8Array(__encode(String(part)));
        chunks.push(bytes);
        total += bytes.length;
      }
      const combined = new Uint8Array(total);
      let offset = 0;
      for (const chunk of chunks) {
        combined.set(chunk, offset);
        offset += chunk.length;
      }
      this.#buffer = combined.buffer;
    }
    get size() {
      return this.#buffer.byteLength;
    }
    get type() {
      return this.#type;
    }
    async arrayBuffer() {
      return this.#buffer.slice(0);
    }
    async text() {
      return __decode(this.#buffer);
    }
    slice(start, end, contentType) {
      const buf = this.#buffer.slice(
        start || 0,
        end !== undefined ? end : this.#buffer.byteLength,
      );
      const b = new Blob([buf]);
      if (contentType) b.#type = String(contentType);
      return b;
    }
    stream() {
      throw new TypeError('ReadableStream is not supported');
    }
  }

  class Headers {
    #map;
    constructor(init) {
      this.#map = new Map();
      if (!init) return;
      if (init instanceof Headers) {
        for (const [k, v] of init) this.#map.set(k, v);
      } else if (Array.isArray(init)) {
        for (const [k, v] of init) this.#map.set(k.toLowerCase(), String(v));
      } else if (typeof init === 'object') {
        for (const [k, v] of Object.entries(init))
          this.#map.set(k.toLowerCase(), String(v));
      }
    }
    append(name, value) {
      const k = name.toLowerCase();
      const cur = this.#map.get(k);
      this.#map.set(k, cur !== undefined ? cur + ', ' + value : String(value));
    }
    delete(name) {
      this.#map.delete(name.toLowerCase());
    }
    get(name) {
      const v = this.#map.get(name.toLowerCase());
      return v !== undefined ? v : null;
    }
    has(name) {
      return this.#map.has(name.toLowerCase());
    }
    set(name, value) {
      this.#map.set(name.toLowerCase(), String(value));
    }
    forEach(cb, thisArg) {
      this.#map.forEach((v, k) => cb.call(thisArg, v, k, this));
    }
    *entries() {
      yield* this.#map;
    }
    *keys() {
      yield* this.#map.keys();
    }
    *values() {
      yield* this.#map.values();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }

  class Response {
    #body;
    #consumed;
    constructor(body, init) {
      if (init === undefined) init = {};
      if (body instanceof ArrayBuffer) this.#body = body;
      else if (body !== null && body !== undefined)
        this.#body = __encode(String(body));
      else this.#body = new ArrayBuffer(0);
      this.#consumed = false;
      this.status = init.status !== undefined ? init.status : 200;
      this.statusText = init.statusText !== undefined ? init.statusText : '';
      this.ok = this.status >= 200 && this.status < 300;
      this.headers =
        init.headers instanceof Headers
          ? init.headers
          : new Headers(init.headers);
      this.type = 'basic';
      this.url = init.url || '';
      this.redirected = !!init.redirected;
    }
    get bodyUsed() {
      return this.#consumed;
    }
    #assertNotConsumed() {
      if (this.#consumed) throw new TypeError('body already consumed');
      this.#consumed = true;
    }
    async text() {
      this.#assertNotConsumed();
      return __decode(this.#body);
    }
    async json() {
      return JSON.parse(await this.text());
    }
    async arrayBuffer() {
      this.#assertNotConsumed();
      return this.#body.slice(0);
    }
    async blob() {
      const buf = await this.arrayBuffer();
      return new Blob([buf], { type: this.headers.get('content-type') || '' });
    }
    async formData() {
      throw new TypeError('formData() is not supported');
    }
    clone() {
      if (this.#consumed)
        throw new TypeError('cannot clone a consumed response');
      return new Response(this.#body.slice(0), {
        status: this.status,
        statusText: this.statusText,
        headers: new Headers(this.headers),
        url: this.url,
        redirected: this.redirected,
      });
    }
  }

  class Request {
    constructor(input, init) {
      if (init === undefined) init = {};
      if (input instanceof Request) {
        this.url = input.url;
        this.method = (init.method || input.method).toUpperCase();
        this.headers = new Headers(init.headers || input.headers);
        this.body = init.body !== undefined ? init.body : input.body;
      } else {
        this.url = String(input);
        this.method = (init.method || 'GET').toUpperCase();
        this.headers = new Headers(init.headers);
        this.body = init.body !== undefined ? init.body : null;
      }
    }
  }

  class FormData {
    #entries;
    constructor() {
      this.#entries = [];
    }
    append(name, value) {
      this.#entries.push([String(name), value]);
    }
    delete(name) {
      this.#entries = this.#entries.filter(([k]) => k !== name);
    }
    get(name) {
      const e = this.#entries.find(([k]) => k === name);
      return e ? e[1] : null;
    }
    getAll(name) {
      return this.#entries.filter(([k]) => k === name).map(([, v]) => v);
    }
    has(name) {
      return this.#entries.some(([k]) => k === name);
    }
    set(name, value) {
      this.delete(name);
      this.append(name, value);
    }
    *entries() {
      yield* this.#entries;
    }
    *keys() {
      for (const [k] of this.#entries) yield k;
    }
    *values() {
      for (const [, v] of this.#entries) yield v;
    }
    forEach(cb, thisArg) {
      for (const [k, v] of this.#entries) cb.call(thisArg, v, k, this);
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }

  globalThis.TextDecoder = TextDecoder;
  globalThis.TextEncoder = TextEncoder;
  globalThis.Blob = Blob;
  globalThis.Headers = Headers;
  globalThis.Response = Response;
  globalThis.Request = Request;
  globalThis.FormData = FormData;

  globalThis.fetch = async function fetch(resource, init) {
    if (init === undefined) init = {};
    let url;
    if (resource instanceof Request) {
      url = resource.url;
      init = {
        method: resource.method,
        headers: Object.fromEntries(resource.headers),
        body: resource.body,
        ...init,
      };
    } else {
      url = String(resource);
    }

    const method = (init.method || 'GET').toUpperCase();
    const headers = {};
    if (init.headers) {
      if (init.headers instanceof Headers) {
        init.headers.forEach((v, k) => {
          headers[k] = v;
        });
      } else if (typeof init.headers === 'object') {
        for (const [k, v] of Object.entries(init.headers)) {
          headers[k.toLowerCase()] = String(v);
        }
      }
    }
    const body = init.body != null ? String(init.body) : null;

    const raw = await __raw(url, method, JSON.stringify(headers), body);
    return new Response(raw.body, raw);
  };
}
