import { StringDecoder } from 'node:string_decoder';
const START = '\x1b[200~';
const END = '\x1b[201~';
function partialMarker(text, marker) {
    for (let length = Math.min(text.length, marker.length - 1); length > 0; length--) {
        if (text.endsWith(marker.slice(0, length)))
            return length;
    }
    return 0;
}
/** Decode terminal bytes without assuming UTF-8 or paste markers fit a chunk. */
export class TerminalPasteDecoder {
    decoder = new StringDecoder('utf8');
    tail = '';
    capturing = false;
    discarding = false;
    chunks = [];
    bytes = 0;
    limit;
    constructor(limit) { this.limit = limit; }
    get hasPendingText() { return !this.capturing && !this.discarding && this.tail.length > 0; }
    get isCapturing() { return this.capturing || this.discarding; }
    flushPendingText() {
        if (!this.hasPendingText)
            return '';
        const text = this.tail;
        this.tail = '';
        return text;
    }
    push(chunk) {
        let raw = this.tail + (typeof chunk === 'string' ? chunk : this.decoder.write(chunk));
        this.tail = '';
        const events = [];
        while (raw.length > 0) {
            if (this.capturing || this.discarding) {
                const end = raw.indexOf(END);
                const keep = end < 0 ? partialMarker(raw, END) : 0;
                const content = end < 0 ? raw.slice(0, raw.length - keep) : raw.slice(0, end);
                if (this.capturing) {
                    this.bytes += Buffer.byteLength(content, 'utf8');
                    if (this.bytes > this.limit) {
                        this.chunks = [];
                        this.capturing = false;
                        this.discarding = true;
                        events.push({ type: 'overflow' });
                    }
                    else
                        this.chunks.push(content);
                }
                if (end < 0) {
                    this.tail = keep ? raw.slice(-keep) : '';
                    break;
                }
                if (this.capturing)
                    events.push({ type: 'paste', text: this.chunks.join('') });
                this.capturing = false;
                this.discarding = false;
                this.chunks = [];
                this.bytes = 0;
                raw = raw.slice(end + END.length);
                continue;
            }
            const start = raw.indexOf(START);
            if (start >= 0) {
                if (start > 0)
                    events.push({ type: 'text', text: raw.slice(0, start) });
                this.capturing = true;
                raw = raw.slice(start + START.length);
                continue;
            }
            // Fallback for terminals that send an unbracketed multiline clipboard
            // block in one large write. Do not echo that block through readline.
            if (raw.length >= 16 * 1024 && /[\r\n]/.test(raw)) {
                events.push(Buffer.byteLength(raw, 'utf8') > this.limit ? { type: 'overflow' } : { type: 'paste', text: raw });
                break;
            }
            const keep = partialMarker(raw, START);
            const text = keep ? raw.slice(0, -keep) : raw;
            if (text)
                events.push({ type: 'text', text });
            this.tail = keep ? raw.slice(-keep) : '';
            break;
        }
        return events;
    }
}
