/**
 * Content-addressed evidence objects for Verified Autonomous Engineering.
 *
 * This module stores bytes, NOT claims about those bytes. Importing a file
 * does not grant permission to read it, certify its source, or prove that the
 * recorded contents are trustworthy. Caller authorization and verifier-owned
 * acceptance remain in the existing Core/goal layers.
 *
 * Layout: root/objects/sha256/aa/<64 lowercase hex digits>
 * Writes: private staging -> fsync -> hard-link no-clobber -> directory fsync.
 *
 * The goal supervisor links these immutable references to ledger evidence;
 * exports re-verify hashes before exposing a package.
 */
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync, closeSync, constants, createReadStream, createWriteStream, existsSync, fsyncSync, linkSync, lstatSync, mkdirSync, openSync, rmSync, statSync, } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { syncDirectory } from '../context/storage.js';
const DEFAULT_MAX_BYTES = 512 * 1024 * 1024;
const SHA256 = /^[0-9a-f]{64}$/;
export class ArtifactIntegrityError extends Error {
    constructor(message) {
        super(message);
        this.name = 'ArtifactIntegrityError';
    }
}
export class ArtifactStore {
    root;
    maxBytes;
    constructor(root, opts = {}) {
        if (!isAbsolute(root))
            throw new TypeError('artifact store root must be absolute');
        const max = opts.maxBytes ?? DEFAULT_MAX_BYTES;
        if (!Number.isSafeInteger(max) || max < 1)
            throw new RangeError('invalid artifact size budget');
        // The store is a private, locally controlled directory. Do not accept a
        // symlinked store root, which could redirect evidence outside its owner.
        mkdirSync(root, { recursive: true, mode: 0o700 });
        if (!lstatSync(root).isDirectory())
            throw new ArtifactIntegrityError('store root is not a directory');
        this.root = root;
        this.maxBytes = max;
        mkdirSync(this.temporaryDir(), { recursive: true, mode: 0o700 });
    }
    temporaryDir() {
        return join(this.root, 'temporary');
    }
    objectPath(digest) {
        if (!SHA256.test(digest))
            throw new TypeError('invalid artifact SHA-256 digest');
        return join(this.root, 'objects', 'sha256', digest.slice(0, 2), digest);
    }
    /** Import only regular files; fail closed on an input symlink. */
    async putFile(input, signal) {
        signal?.throwIfAborted();
        const info = lstatSync(input);
        if (!info.isFile())
            throw new ArtifactIntegrityError('artifact source must be a regular file (no symlinks)');
        if (info.size > this.maxBytes)
            throw new RangeError('artifact exceeds size budget');
        // O_NOFOLLOW closes the race in which a regular file changes to a symlink
        // after the lstat. Other changes to file content are represented by the
        // bytes actually read and hashed, not by filename or initial file metadata.
        const source = createReadStream(input, {
            flags: (constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0)),
            highWaterMark: 64 * 1024,
        });
        return this.importStream(source, signal);
    }
    async putBytes(bytes, signal) {
        signal?.throwIfAborted();
        if (bytes.byteLength > this.maxBytes)
            throw new RangeError('artifact exceeds size budget');
        return this.importStream(Readable.from([bytes]), signal);
    }
    async importStream(source, signal) {
        const temp = join(this.temporaryDir(), randomUUID() + '.partial');
        const hash = createHash('sha256');
        let sizeBytes = 0;
        let published = false;
        try {
            const meter = new Transform({
                transform: (raw, _encoding, callback) => {
                    const chunk = Buffer.isBuffer(raw) ? raw : Buffer.from(raw);
                    sizeBytes += chunk.byteLength;
                    if (sizeBytes > this.maxBytes) {
                        callback(new RangeError('artifact exceeds size budget'));
                        return;
                    }
                    hash.update(chunk);
                    callback(null, chunk);
                },
            });
            await pipeline(source, meter, createWriteStream(temp, { flags: 'wx', mode: 0o600 }), { signal });
            const digest = hash.digest('hex');
            const target = this.objectPath(digest);
            const dir = join(this.root, 'objects', 'sha256', digest.slice(0, 2));
            mkdirSync(dir, { recursive: true, mode: 0o700 });
            // Ensure the object bytes are on disk before publishing their pathname.
            let fd;
            try {
                fd = openSync(temp, 'r');
                fsyncSync(fd);
            }
            finally {
                if (fd !== undefined)
                    closeSync(fd);
            }
            chmodSync(temp, 0o400);
            try {
                linkSync(temp, target);
                published = true;
                syncDirectory(dir);
            }
            catch (error) {
                if (error.code !== 'EEXIST')
                    throw error;
                // Never overwrite a preexisting digest path, even when damaged. An
                // existing corrupt object is an integrity incident, not a cache hit.
                if (!(await this.verify(digest, sizeBytes))) {
                    throw new ArtifactIntegrityError('existing artifact object failed SHA-256 verification: ' + digest);
                }
            }
            return { algorithm: 'sha256', sha256: digest, sizeBytes, path: target, created: published };
        }
        finally {
            rmSync(temp, { force: true });
        }
    }
    /** Locate only well-formed digest names. The returned path is NOT proof. */
    locate(digest) {
        return this.objectPath(digest);
    }
    /** Read and hash the actual bytes; metadata, filenames and model text alone
     * are insufficient to establish integrity. */
    async verify(digest, expectedSize) {
        const path = this.objectPath(digest);
        if (!existsSync(path))
            return false;
        try {
            if (!lstatSync(path).isFile())
                return false;
            if (expectedSize !== undefined && statSync(path).size !== expectedSize)
                return false;
            const hash = createHash('sha256');
            let total = 0;
            for await (const chunk of createReadStream(path, {
                flags: (constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0)),
                highWaterMark: 64 * 1024,
            })) {
                total += chunk.byteLength;
                if (total > this.maxBytes)
                    return false;
                hash.update(chunk);
            }
            return (expectedSize === undefined || total === expectedSize) && hash.digest('hex') === digest;
        }
        catch (error) {
            if (['ENOENT', 'ELOOP'].includes(error.code ?? ''))
                return false;
            throw error;
        }
    }
    /** Return bytes only after verifying their content address and regular-file status. */
    async readVerified(digest) {
        const path = this.objectPath(digest);
        if (!(await this.verify(digest)))
            throw new ArtifactIntegrityError(`artifact object failed SHA-256 verification: ${digest}`);
        const info = lstatSync(path);
        if (!info.isFile() || info.size > this.maxBytes)
            throw new ArtifactIntegrityError(`artifact object is not a bounded regular file: ${digest}`);
        const bytes = await import('node:fs/promises').then(fs => fs.readFile(path));
        if (createHash('sha256').update(bytes).digest('hex') !== digest)
            throw new ArtifactIntegrityError(`artifact changed while reading: ${digest}`);
        return bytes;
    }
}
