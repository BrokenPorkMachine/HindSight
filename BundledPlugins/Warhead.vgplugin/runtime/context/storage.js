import { closeSync, fsyncSync, mkdirSync, openSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';
/** Replace saved state atomically; failed writes leave the previous copy intact. */
export function writePrivateJson(path, value) {
    const contents = JSON.stringify(value, null, 2) + '\n';
    writePrivateText(path, contents);
}
export function writePrivateText(path, contents) {
    mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
    const temporary = `${path}.${randomUUID()}.tmp`;
    try {
        const fd = openSync(temporary, 'wx', 0o600);
        try {
            writeFileSync(fd, contents, 'utf8');
            fsyncSync(fd);
        }
        finally {
            closeSync(fd);
        }
        renameSync(temporary, path);
        syncDirectory(dirname(path));
    }
    finally {
        rmSync(temporary, { force: true });
    }
}
/** Persist a rename on filesystems supporting directory fsync. */
export function syncDirectory(path) {
    let fd;
    try {
        fd = openSync(path, 'r');
        fsyncSync(fd);
    }
    catch (error) {
        if (!['EINVAL', 'ENOTSUP', 'EBADF'].includes(error.code ?? ''))
            throw error;
    }
    finally {
        if (fd !== undefined)
            closeSync(fd);
    }
}
