{
    const __rawStat = globalThis.fs.__rawStat;
    delete globalThis.fs.__rawStat;

    class Stats {
        #d;
        constructor(d) { this.#d = d; }
        isFile() { return this.#d.isFile; }
        isDirectory() { return this.#d.isDirectory; }
        isSymbolicLink() { return this.#d.isSymlink; }
        isBlockDevice() { return false; }
        isCharacterDevice() { return false; }
        isFIFO() { return false; }
        isSocket() { return false; }
        get dev() { return this.#d.dev; }
        get ino() { return this.#d.ino; }
        get mode() { return this.#d.mode; }
        get nlink() { return this.#d.nlink; }
        get uid() { return this.#d.uid; }
        get gid() { return this.#d.gid; }
        get size() { return this.#d.size; }
        get atimeMs() { return this.#d.atimeMs; }
        get mtimeMs() { return this.#d.mtimeMs; }
        get ctimeMs() { return this.#d.ctimeMs; }
        get birthtimeMs() { return this.#d.birthtimeMs; }
        get atime() { return new Date(this.#d.atimeMs); }
        get mtime() { return new Date(this.#d.mtimeMs); }
        get ctime() { return new Date(this.#d.ctimeMs); }
        get birthtime() { return new Date(this.#d.birthtimeMs); }
    }

    globalThis.fs.stat = async (path) => new Stats(await __rawStat(path));
    globalThis.fs.lstat = globalThis.fs.stat;
}
