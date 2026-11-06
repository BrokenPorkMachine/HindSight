import { basename, extname } from 'node:path';
import { statSync, openSync, readSync, closeSync, readFileSync, existsSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { MAX_ATTACHMENT_BYTES } from '../context/session.js';
import { humanBytes } from '../context/transcript.js';
export function formatAttachment(attachment) {
    const name = attachment.name.replace(/[&<>\"]/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[char]);
    const details = [attachment.kind, attachment.language, attachment.detectedType].filter(Boolean).join('; ').replace(/[&<>\"]/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[char]);
    const boundary = randomUUID();
    return [
        `<attachment name="${name}"${details ? ` type="${details}"` : ''} size="${attachment.bytes} bytes" boundary="${boundary}">`,
        `--- BEGIN ATTACHMENT ${boundary} ---`,
        attachment.content,
        `--- END ATTACHMENT ${boundary} ---`,
    ].join('\n');
}
const SOURCE_LANGUAGES = {
    c: 'C', h: 'C/C++ header', cc: 'C++', cpp: 'C++', cxx: 'C++', hpp: 'C++ header',
    cs: 'C#', go: 'Go', rs: 'Rust', py: 'Python', pyw: 'Python', js: 'JavaScript',
    jsx: 'JavaScript/JSX', mjs: 'JavaScript', cjs: 'JavaScript', ts: 'TypeScript',
    tsx: 'TypeScript/JSX', java: 'Java', kt: 'Kotlin', swift: 'Swift', rb: 'Ruby',
    php: 'PHP', sh: 'Shell', bash: 'Shell', zsh: 'Shell', fish: 'Fish shell',
    sql: 'SQL', html: 'HTML', htm: 'HTML', css: 'CSS', scss: 'SCSS', less: 'Less',
    json: 'JSON', jsonc: 'JSON with comments', yaml: 'YAML', yml: 'YAML', toml: 'TOML',
    xml: 'XML', md: 'Markdown', markdown: 'Markdown', vue: 'Vue', svelte: 'Svelte',
    proto: 'Protocol Buffers', graphql: 'GraphQL', gql: 'GraphQL', ipynb: 'Jupyter notebook',
    dockerfile: 'Dockerfile', makefile: 'Makefile', tf: 'Terraform', bicep: 'Bicep',
    diff: 'Diff/patch', patch: 'Diff/patch',
};
function detectBinaryType(bytes) {
    if (bytes.length < 4)
        return 'binary data (format not identified)';
    if (bytes.length >= 4 && bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])))
        return 'ELF executable/object';
    if (bytes.length >= 2 && bytes[0] === 0x4d && bytes[1] === 0x5a)
        return 'Windows PE executable or DLL';
    const magic = bytes.readUInt32BE(0);
    if ([0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe, 0xcafebabe, 0xcafebabf].includes(magic))
        return 'Mach-O executable/object';
    if (bytes.subarray(0, 4).toString() === '%PDF')
        return 'PDF document';
    if (bytes.subarray(0, 4).equals(Buffer.from([0x50, 0x4b, 0x03, 0x04])))
        return 'ZIP archive or ZIP-based document';
    if (bytes.subarray(0, 3).toString() === 'SQLite format 3')
        return 'SQLite database';
    if (bytes[0] === 0x1f && bytes[1] === 0x8b)
        return 'gzip compressed data';
    if (bytes.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])))
        return 'PNG image';
    if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff)
        return 'JPEG image';
    return 'binary data (format not identified)';
}
function isBinary(bytes) {
    if (bytes.includes(0))
        return true;
    const sample = bytes.subarray(0, Math.min(bytes.length, 8192));
    let controls = 0;
    for (const byte of sample)
        if (byte < 0x20 && byte !== 9 && byte !== 10 && byte !== 12 && byte !== 13)
            controls++;
    return sample.length > 0 && controls / sample.length > 0.02;
}
export function loadAttachment(path) {
    const stat = statSync(path);
    if (!stat.isFile())
        throw new Error('only regular files can be attached');
    const base = basename(path).toLowerCase();
    const extension = extname(base).slice(1) || base;
    const language = SOURCE_LANGUAGES[extension];
    const fd = openSync(path, 'r');
    const sampleBuffer = Buffer.alloc(Math.min(stat.size, 8192));
    try {
        readSync(fd, sampleBuffer, 0, sampleBuffer.length, 0);
    }
    finally {
        closeSync(fd);
    }
    const sample = sampleBuffer;
    if (isBinary(sample)) {
        const detectedType = detectBinaryType(sample);
        const preview = sample.subarray(0, 32).toString('hex').match(/.{1,2}/g)?.join(' ') ?? '(empty)';
        return {
            name: path, bytes: stat.size, kind: 'binary', detectedType,
            content: `Binary file metadata only; contents are not decoded or sent to the model.\nDetected format: ${detectedType}.\nFirst ${Math.min(stat.size, 32)} bytes (hex): ${preview}\nInspect the original file with available tools if the user asks for deeper analysis.`,
        };
    }
    if (stat.size > MAX_ATTACHMENT_BYTES) {
        return {
            name: path, bytes: stat.size, kind: 'metadata', language,
            content: `Text file omitted because it exceeds the ${humanBytes(MAX_ATTACHMENT_BYTES)} inline limit. The original path is available to inspect with tools.`,
        };
    }
    const bytes = readFileSync(path);
    return {
        name: path, bytes: bytes.length, kind: language ? 'source' : 'text', language,
        content: bytes.toString('utf8'),
    };
}
export function extractDroppedPaths(line) {
    try {
        if (existsSync(line) && statSync(line).isFile())
            return { text: '', paths: [line] };
    }
    catch { /* fall through to token recognition */ }
    const tokenPattern = /(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|(?:\\.|[^\s])+)/g;
    const paths = [];
    const kept = [];
    let cursor = 0;
    for (const match of line.matchAll(tokenPattern)) {
        const token = match[0];
        let path = token;
        if ((path.startsWith('"') && path.endsWith('"')) || (path.startsWith("'") && path.endsWith("'")))
            path = path.slice(1, -1);
        path = path.replace(/\\(.)/g, '$1');
        try {
            if (!existsSync(path) || !statSync(path).isFile())
                continue;
        }
        catch {
            continue;
        }
        const index = match.index;
        kept.push(line.slice(cursor, index));
        cursor = index + token.length;
        paths.push(path);
    }
    if (paths.length === 0)
        return { text: line, paths };
    kept.push(line.slice(cursor));
    return { text: kept.join(' ').replace(/\s+/g, ' ').trim(), paths };
}
