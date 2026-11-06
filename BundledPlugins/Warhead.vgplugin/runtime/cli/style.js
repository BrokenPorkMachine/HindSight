import { basename } from 'node:path';
import { terminalCellWidth } from './terminal.js';
import { humanBytes } from '../context/transcript.js';
export const COLOR = Boolean(process.stdout.isTTY && !process.env['NO_COLOR']);
export const RESET = COLOR ? '\x1b[0m' : '';
export const DIM = COLOR ? '\x1b[2m' : '';
export const BOLD = COLOR ? '\x1b[1m' : '';
export const CYAN = COLOR ? '\x1b[36m' : '';
export const GREEN = COLOR ? '\x1b[32m' : '';
export const YELLOW = COLOR ? '\x1b[33m' : '';
export const RED = COLOR ? '\x1b[31m' : '';
export const MAGENTA = COLOR ? '\x1b[35m' : '';
// Once the readline editor is live, output must pass through the prompt-aware
// writer. Direct writes leave readline's editor painted underneath streamed
// model/tool output, which makes the prompt appear to overlap the response.
export let promptAwareWrite = null;
export const out = (s = '') => {
    const text = `${s}\n`;
    if (promptAwareWrite)
        promptAwareWrite(text);
    else
        process.stdout.write(text);
};
export function compactText(value, max = 180) {
    const oneLine = value.replace(/\s+/g, ' ').trim();
    return oneLine.length > max ? `${oneLine.slice(0, max - 1)}…` : oneLine;
}
export function terminalWidth() {
    return Math.max(24, Math.min(110, (process.stdout.columns || 88) - 2));
}
export function terminalHeight() { return Math.max(6, process.stdout.rows || 24); }
export function uiRule(width, title) {
    return '─'.repeat(Math.max(1, width - title.length - 5));
}
export function displayName(path, max = 42) {
    const name = basename(path);
    return name.length > max ? `…${name.slice(-(max - 1))}` : name;
}
export function responsePreview(text, max = 180) {
    return compactText(text.replace(/\n/g, ' ↵ '), max);
}
export function attachmentPreview(attachments, max = 3, nameMax = 42, maxChars = Number.POSITIVE_INFINITY) {
    if (attachments.length === 0)
        return 'no attachments queued';
    const totalBytes = attachments.reduce((sum, attachment) => sum + attachment.bytes, 0);
    const count = `${attachments.length} attachment${attachments.length === 1 ? '' : 's'} · ${humanBytes(totalBytes)}`;
    const shown = attachments.slice(0, max).map(a => `📎 ${displayName(a.name, nameMax)} · ${humanBytes(a.bytes)}`);
    if (attachments.length > max)
        shown.push(`+${attachments.length - max} more`);
    const detail = `${count}  ${shown.join('  ')}`;
    if (terminalCellWidth(detail) <= maxChars)
        return detail;
    const compact = `${count}  ${shown[0] ?? ''}${attachments.length > 1 ? `  +${attachments.length - 1} more` : ''}`;
    if (terminalCellWidth(compact) <= maxChars)
        return compact;
    return terminalCellWidth(count) <= maxChars ? count : `${count.slice(0, Math.max(1, maxChars - 1))}…`;
}
export function setPromptWriter(writer) { promptAwareWrite = writer; }
