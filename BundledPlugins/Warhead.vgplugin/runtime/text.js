import { StringDecoder } from 'node:string_decoder';
/** Return a UTF-8 byte-bounded prefix without a partial final code point. */
export function utf8Prefix(text, maxBytes) {
    const bytes = Buffer.from(text, 'utf8');
    if (bytes.length <= maxBytes)
        return text;
    return new StringDecoder('utf8').write(bytes.subarray(0, Math.max(0, maxBytes)));
}
