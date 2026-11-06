/** Durable project writes with a private preimage saved before replacement. */
import { closeSync, constants, fchmodSync, fstatSync, fsyncSync, linkSync, lstatSync, mkdirSync, openSync, readSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { transcriptDir } from '../config.js';
import { syncDirectory, writePrivateJson } from '../context/storage.js';
import { pruneRecovery } from '../context/retention.js';
export const digest = (contents) => createHash('sha256').update(contents).digest('hex');
const MAX_PROJECT_BYTES = 5 * 1024 * 1024;
function readProjectBytes(path) {
    const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    try {
        const stat = fstatSync(fd);
        if (!stat.isFile() || stat.size > MAX_PROJECT_BYTES)
            throw new Error(`${path} is not a regular file within the 5 MiB checkpoint limit`);
        const bytes = Buffer.alloc(stat.size + 1);
        let length = 0;
        while (length < bytes.length) {
            const read = readSync(fd, bytes, length, bytes.length - length, length);
            if (!read)
                break;
            length += read;
        }
        if (length !== stat.size || fstatSync(fd).size !== stat.size)
            throw new Error(`${path} changed while being read; try again`);
        return bytes.subarray(0, length);
    }
    finally {
        closeSync(fd);
    }
}
export function replaceProjectFile(path, contents, ctx, options = {}) {
    ctx.signal.throwIfAborted();
    if (Buffer.byteLength(contents) > MAX_PROJECT_BYTES)
        throw new Error('Replacement exceeds the 5 MiB file limit');
    let before = null;
    let mode = 0o666 & ~process.umask();
    let identity;
    try {
        identity = lstatSync(path);
        if (options.createOnly)
            throw new Error(`${path} already exists and overwrite was not set.`);
        if (!identity.isFile())
            throw new Error(`Refusing to replace non-regular file ${path}; use its real file path`);
        before = readProjectBytes(path);
        mode = identity.mode & 0o777;
    }
    catch (error) {
        if (error.code !== 'ENOENT')
            throw error;
    }
    if (options.expected !== undefined && (!before || !before.equals(Buffer.from(options.expected))))
        throw new Error(`${path} changed since it was read; read it again before editing`);
    mkdirSync(dirname(path), { recursive: true });
    const temporary = join(dirname(path), `.warhead-${randomUUID()}.tmp`);
    try {
        const fd = openSync(temporary, 'wx', mode);
        try {
            // open() applies the process umask. The replacement must keep the exact
            // mode of the file it replaces, including group/other executable bits.
            fchmodSync(fd, mode);
            writeFileSync(fd, contents, 'utf8');
            fsyncSync(fd);
        }
        finally {
            closeSync(fd);
        }
        ctx.signal.throwIfAborted();
        const checkpoint = join(ctx.recoveryDir ?? join(transcriptDir(), '..', 'recovery', 'files'), `${Date.now()}-${randomUUID()}.json`);
        writePrivateJson(checkpoint, {
            version: 1, path, savedAt: new Date().toISOString(), mode,
            before: before === null ? null : before.toString('base64'), afterHash: digest(contents),
        });
        pruneRecovery(dirname(checkpoint), checkpoint);
        // Catch concurrent editor changes before committing our staged replacement.
        if (identity) {
            const current = lstatSync(path);
            if (!current.isFile() || current.ino !== identity.ino || current.dev !== identity.dev || digest(readProjectBytes(path)) !== digest(before))
                throw new Error(`${path} changed during the edit; original left untouched`);
        }
        ctx.signal.throwIfAborted();
        if (options.createOnly || before === null)
            linkSync(temporary, path);
        else
            renameSync(temporary, path);
        syncDirectory(dirname(path));
        return checkpoint;
    }
    finally {
        rmSync(temporary, { force: true });
    }
}
